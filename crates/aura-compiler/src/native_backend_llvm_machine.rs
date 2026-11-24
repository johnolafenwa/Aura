//! Freestanding entry conventions and linker-visible placement.
use super::*;
use crate::machine_attributes::Interrupt;
use std::fmt::Write;

/// Apply the declaration contract after every LLVM body exists and before
/// optimization, so an interrupt wrapper can refer to its ordinary callee.
pub(super) fn apply(lowerer: &ObjectLowerer<'_, '_>, object: &LlvmModule) -> Result<(), String> {
    let mut retained = Vec::new();
    for &index in &lowerer.bodies {
        let decl = &object.functions[index];
        let Some(attrs) = &decl.machine else { continue };
        let mut entry = lowerer.functions[index];
        if let Some(link_name) = &attrs.link_name {
            if entry.get_name().to_bytes() != link_name.as_bytes() {
                if lowerer.module.get_function(link_name).is_some()
                    || lowerer.module.get_global(link_name).is_some()
                {
                    return Err(format!("link name `{link_name}` is already defined"));
                }
                entry.as_global_value().set_name(link_name);
            }
        }
        if let Some(irq) = attrs.interrupt {
            let target = object
                .freestanding
                .ok_or("interrupt entry requires a freestanding object")?;
            if target.arch() != irq.arch() {
                return Err(format!("interrupt entry requires {}", irq.arch()));
            }
            let symbol = entry.get_name().to_string_lossy().into_owned();
            let linkage = entry.get_linkage();
            entry
                .as_global_value()
                .set_name(&format!("{symbol}.handler"));
            entry.set_linkage(LlvmLinkage::Internal);
            let handler = entry;
            entry = lowerer.module.add_function(
                &symbol,
                lowerer.context.void_type().fn_type(&[], false),
                Some(linkage),
            );
            // A literal symbol operand keeps the body live without giving
            // LLVM freedom to materialize its address before the register saves.
            let assembly =
                interrupt_assembly(irq, target == super::super::FreestandingTarget::X86_64);
            build_naked(lowerer.context, entry, assembly, Some(handler))?;
        } else if let Some(assembly) = &attrs.naked {
            build_naked(lowerer.context, entry, assembly.clone(), None)?;
        }
        if let Some(inline) = attrs.inline {
            let (add, remove) = if inline {
                ("alwaysinline", "noinline")
            } else {
                ("noinline", "alwaysinline")
            };
            entry.remove_enum_attribute(
                AttributeLoc::Function,
                Attribute::get_named_enum_kind_id(remove),
            );
            attr(lowerer.context, entry, add);
        }
        if let Some(section) = &attrs.section {
            set_section(entry, section);
        }
        if let Some(align) = attrs.align {
            entry.as_global_value().set_alignment(align);
        }
        if attrs.used {
            retained.push(entry.as_global_value().as_pointer_value());
        }
    }
    for (global, decl) in lowerer.globals.iter().zip(&object.data) {
        if let Some(place) = decl.contents.as_ref().and_then(|c| c.placement.as_ref()) {
            if let Some(section) = &place.section {
                let name = std::ffi::CString::new(section.as_str()).map_err(|e| e.to_string())?;
                // SAFETY: global belongs to this module, and LLVM copies its section name.
                unsafe {
                    inkwell::llvm_sys::core::LLVMSetSection(global.as_value_ref(), name.as_ptr())
                };
            }
            retained.push(global.as_pointer_value());
        }
    }
    if !retained.is_empty() {
        let ptr = lowerer.context.ptr_type(AddressSpace::default());
        let array = ptr.const_array(&retained);
        let used = lowerer
            .module
            .add_global(array.get_type(), None, "llvm.used");
        used.set_initializer(&array);
        used.set_linkage(LlvmLinkage::Appending);
        // inkwell applies host Mach-O section spelling even when emitting ELF.
        // SAFETY: the module owns this global and LLVM copies the literal.
        unsafe {
            inkwell::llvm_sys::core::LLVMSetSection(used.as_value_ref(), c"llvm.metadata".as_ptr())
        };
    }
    Ok(())
}

fn attr(context: &Context, function: FunctionValue<'_>, name: &str) {
    function.add_attribute(
        AttributeLoc::Function,
        context.create_enum_attribute(Attribute::get_named_enum_kind_id(name), 0),
    );
}

fn build_naked<'ctx>(
    context: &'ctx Context,
    entry: FunctionValue<'ctx>,
    assembly: String,
    callee: Option<FunctionValue<'ctx>>,
) -> Result<(), String> {
    entry.remove_enum_attribute(
        AttributeLoc::Function,
        Attribute::get_named_enum_kind_id("alwaysinline"),
    );
    attr(context, entry, "naked");
    attr(context, entry, "noinline");
    let pointer = context.ptr_type(AddressSpace::default());
    let (ty, constraint, args) = if let Some(callee) = callee {
        (
            context.void_type().fn_type(&[pointer.into()], false),
            "s",
            vec![callee.as_global_value().as_pointer_value().into()],
        )
    } else {
        (context.void_type().fn_type(&[], false), "", vec![])
    };
    let asm = context.create_inline_asm(
        ty,
        assembly,
        constraint.to_owned(),
        true,
        false,
        None,
        false,
    );
    let builder = context.create_builder();
    builder.position_at_end(context.append_basic_block(entry, "entry"));
    builder
        .build_indirect_call(ty, asm, &args, "")
        .map_err(|e| e.to_string())?;
    builder.build_unreachable().map_err(|e| e.to_string())?;
    Ok(())
}

fn interrupt_assembly(irq: Interrupt, x86_float: bool) -> String {
    match irq {
        Interrupt::X86 | Interrupt::X86Error => x86_entry(irq == Interrupt::X86Error, x86_float),
        Interrupt::Aarch64 => arm_entry(),
        Interrupt::Riscv => riscv_entry(),
    }
}

/// Save the SysV caller-saved registers. RBX holds the interrupted stack
/// pointer across the ordinary call and is itself saved on entry.
fn x86_entry(error: bool, float: bool) -> String {
    let regs = [
        "rax", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11", "rbx",
    ];
    let mut text = String::new();
    for r in regs {
        writeln!(text, "pushq %{r}").unwrap();
    }
    text.push_str("movq %rsp, %rbx\nandq $$-16, %rsp\ncld\n");
    if float {
        text.push_str("subq $$512, %rsp\nfxsave64 (%rsp)\n");
    }
    if error {
        writeln!(text, "movq {}(%rbx), %rdi", regs.len() * 8).unwrap();
    }
    text.push_str("callq ${0:c}\n");
    if float {
        text.push_str("fxrstor64 (%rsp)\n");
    }
    text.push_str("movq %rbx, %rsp\n");
    for r in regs.into_iter().rev() {
        writeln!(text, "popq %{r}").unwrap();
    }
    if error {
        text.push_str("addq $$8, %rsp\n");
    }
    text.push_str("iretq");
    text
}

/// EL1h vectors use the current EL1 stack. Preserve caller-saved GPRs,
/// LR, complete vector registers and the privileged exception return state.
fn arm_entry() -> String {
    let mut text = String::from("sub sp, sp, #704\n");
    for i in (0..18).step_by(2) {
        writeln!(text, "stp x{i}, x{}, [sp, #{}]", i + 1, i * 8).unwrap();
    }
    text.push_str("stp x18, x30, [sp, #144]\nmrs x0, elr_el1\nmrs x1, spsr_el1\nstp x0, x1, [sp, #160]\nmrs x0, fpcr\nmrs x1, fpsr\nstp x0, x1, [sp, #176]\n");
    for i in (0..32).step_by(2) {
        writeln!(text, "stp q{i}, q{}, [sp, #{}]", i + 1, 192 + i * 16).unwrap();
    }
    text.push_str("bl ${0:c}\n");
    for i in (0..32).step_by(2) {
        writeln!(text, "ldp q{i}, q{}, [sp, #{}]", i + 1, 192 + i * 16).unwrap();
    }
    text.push_str("ldp x0, x1, [sp, #176]\nmsr fpcr, x0\nmsr fpsr, x1\nldp x0, x1, [sp, #160]\nmsr elr_el1, x0\nmsr spsr_el1, x1\nldp x18, x30, [sp, #144]\n");
    for i in (0..18).step_by(2) {
        writeln!(text, "ldp x{i}, x{}, [sp, #{}]", i + 1, i * 8).unwrap();
    }
    text.push_str("add sp, sp, #704\neret");
    text
}

/// Machine-mode vectors enter with a valid LP64D stack. FS=Off has no
/// live floating state. Enable FP for the handler and restore FS on exit.
fn riscv_entry() -> String {
    let regs = [
        "ra", "t0", "t1", "t2", "t3", "t4", "t5", "t6", "a0", "a1", "a2", "a3", "a4", "a5", "a6",
        "a7",
    ];
    let mut text = String::from("addi sp, sp, -416\n");
    for (i, r) in regs.iter().enumerate() {
        writeln!(text, "sd {r}, {}(sp)", i * 8).unwrap();
    }
    text.push_str("csrr t0, mepc\nsd t0, 128(sp)\ncsrr t0, mstatus\nsd t0, 136(sp)\nli t1, 24576\ncsrs mstatus, t1\nand t0, t0, t1\nbeqz t0, 2f\nfrcsr t0\nsd t0, 144(sp)\n");
    for i in 0..32 {
        writeln!(text, "fsd f{i}, {}(sp)", 160 + i * 8).unwrap();
    }
    text.push_str("2:\ncall ${0:c}\nld t0, 136(sp)\nli t1, 24576\nand t1, t0, t1\nbeqz t1, 3f\n");
    for i in 0..32 {
        writeln!(text, "fld f{i}, {}(sp)", 160 + i * 8).unwrap();
    }
    text.push_str(
        "ld t1, 144(sp)\nfscsr t1\n3:\ncsrw mstatus, t0\nld t1, 128(sp)\ncsrw mepc, t1\n",
    );
    for (i, r) in regs.iter().enumerate() {
        writeln!(text, "ld {r}, {}(sp)", i * 8).unwrap();
    }
    text.push_str("addi sp, sp, 416\nmret");
    text
}
