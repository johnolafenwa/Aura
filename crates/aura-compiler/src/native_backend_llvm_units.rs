//! Codegen units: how a large object splits into LLVM modules that
//! threads optimize and write in parallel, as rustc's codegen units do.
//!
//! The split depends only on the recorded functions, in the order the
//! emitter declared them, so one program always splits the same way and
//! every unit's object holds the same bytes whichever thread wrote it.
//!
//! - A module under `SPLIT_WEIGHT` recorded instructions stays one unit,
//!   and that unit is exactly the object an unsplit module writes.
//! - A larger module splits into runs of consecutive functions of about
//!   `UNIT_WEIGHT` instructions each, at most `MAX_UNITS`. Consecutive
//!   functions are usually one source function and its thunks, or
//!   neighbours in the source.
//! - A local function or data object that another unit refers to becomes
//!   a hidden symbol, so the linker joins the units and the executable
//!   exports nothing new.
//! - A unit that calls a small function of another unit, or one the
//!   emitter marked for inlining, gets an `available_externally` copy, so
//!   LLVM can still inline it. A unit that reads constant data of another
//!   unit gets a copy too, so LLVM can still fold its loads. LLVM drops
//!   every copy before it writes the object.

use std::collections::BTreeSet;

use super::LlvmModule;
use crate::native_backend::DataAddress;

/// A module of fewer recorded instructions stays one unit.
pub(super) const SPLIT_WEIGHT: usize = 4_000;

/// The recorded instructions one unit aims for.
pub(super) const UNIT_WEIGHT: usize = 3_000;

/// The most units a module splits into.
pub(super) const MAX_UNITS: usize = 16;

/// A function of another unit with at most this many recorded
/// instructions gets a copy in each unit that calls it.
pub(super) const COPY_WEIGHT: usize = 64;

/// How one unit holds a function or data object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Placement {
    /// Defined in this unit, and no other unit refers to it.
    Private,
    /// Defined in this unit, and another unit refers to it.
    Shared,
    /// Defined in another unit, with a copy here for LLVM to inline or
    /// fold.
    Copy,
    /// Defined in another unit, or in no unit.
    Elsewhere,
}

/// Which unit defines each function and data object, and what each unit
/// shares and copies.
pub(super) struct Partition {
    units: usize,
    function_unit: Vec<Option<usize>>,
    data_unit: Vec<Option<usize>>,
    function_shared: Vec<bool>,
    data_shared: Vec<bool>,
    function_copies: Vec<BTreeSet<usize>>,
    data_copies: Vec<BTreeSet<usize>>,
    weights: Vec<usize>,
}

impl Partition {
    /// One unit that defines everything.
    pub(super) fn whole(object: &LlvmModule) -> Self {
        Self::with_units(object, 1)
    }

    /// The units `object` splits into.
    pub(super) fn split(object: &LlvmModule) -> Self {
        let total = object.functions.iter().map(function_weight).sum::<usize>();
        Self::with_units(object, unit_count(total))
    }

    fn with_units(object: &LlvmModule, units: usize) -> Self {
        let weights = object
            .functions
            .iter()
            .map(function_weight)
            .collect::<Vec<_>>();
        let total = weights.iter().sum::<usize>().max(1);
        let mut start = 0;
        let mut function_unit = object
            .functions
            .iter()
            .zip(&weights)
            .map(|(decl, weight)| {
                decl.body.as_ref()?;
                // A function belongs to the unit its first instruction
                // falls in.
                let unit = (start * units / total).min(units - 1);
                start += weight;
                Some(unit)
            })
            .collect::<Vec<_>>();
        // A function larger than a unit leaves the units it covers empty.
        // The units that hold functions are numbered again, in order.
        let mut numbers = vec![None; units];
        let mut units = 0;
        for unit in function_unit.iter_mut().flatten() {
            *unit = *numbers[*unit].get_or_insert_with(|| {
                units += 1;
                units - 1
            });
        }
        let units = units.max(1);
        // Data belongs to the first unit that refers to it.
        let mut data_unit = object
            .data
            .iter()
            .map(|decl| decl.contents.as_ref().map(|_| usize::MAX))
            .collect::<Vec<_>>();
        for (decl, unit) in object.functions.iter().zip(&function_unit) {
            let (Some(body), Some(unit)) = (&decl.body, unit) else {
                continue;
            };
            for data in &body.data_refs {
                if let Some(owner) = &mut data_unit[data.0 as usize] {
                    *owner = (*owner).min(*unit);
                }
            }
        }
        for owner in data_unit.iter_mut().flatten() {
            if *owner == usize::MAX {
                *owner = 0;
            }
        }
        let mut partition = Self {
            units,
            function_unit,
            data_unit,
            function_shared: vec![false; object.functions.len()],
            data_shared: vec![false; object.data.len()],
            function_copies: vec![BTreeSet::new(); units],
            data_copies: vec![BTreeSet::new(); units],
            weights,
        };
        if units > 1 {
            for unit in 0..units {
                partition.link_unit(object, unit);
            }
        }
        partition
    }

    /// Records what `unit` refers to in other units: those become shared,
    /// and the small or constant ones get copies in `unit`.
    fn link_unit(&mut self, object: &LlvmModule, unit: usize) {
        let mut functions = (0..object.functions.len())
            .filter(|function| self.function_unit[*function] == Some(unit))
            .collect::<Vec<_>>();
        let mut data = (0..object.data.len())
            .filter(|data| self.data_unit[*data] == Some(unit))
            .collect::<Vec<_>>();
        while !functions.is_empty() || !data.is_empty() {
            while let Some(function) = functions.pop() {
                let Some(body) = &object.functions[function].body else {
                    continue;
                };
                for callee in called_functions(body) {
                    if self.refer_to_function(unit, callee)
                        && (object.functions[callee].inline || self.weights[callee] <= COPY_WEIGHT)
                        && self.function_copies[unit].insert(callee)
                    {
                        functions.push(callee);
                    }
                }
                for target in &body.data_refs {
                    let target = target.0 as usize;
                    if self.refer_to_data(unit, target)
                        && !object.data[target].writable
                        && self.data_copies[unit].insert(target)
                    {
                        data.push(target);
                    }
                }
            }
            while let Some(index) = data.pop() {
                let Some(contents) = &object.data[index].contents else {
                    continue;
                };
                for (_, address) in &contents.addresses {
                    match address {
                        DataAddress::Function(function) => {
                            self.refer_to_function(unit, function.0 as usize);
                        }
                        DataAddress::Data(target) => {
                            let target = target.0 as usize;
                            if self.refer_to_data(unit, target)
                                && !object.data[target].writable
                                && self.data_copies[unit].insert(target)
                            {
                                data.push(target);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Marks `function` shared when another unit defines it, and says so.
    fn refer_to_function(&mut self, unit: usize, function: usize) -> bool {
        match self.function_unit[function] {
            Some(owner) if owner != unit => {
                self.function_shared[function] = true;
                true
            }
            _ => false,
        }
    }

    /// Marks `data` shared when another unit defines it, and says so.
    fn refer_to_data(&mut self, unit: usize, data: usize) -> bool {
        match self.data_unit[data] {
            Some(owner) if owner != unit => {
                self.data_shared[data] = true;
                true
            }
            _ => false,
        }
    }

    pub(super) fn units(&self) -> usize {
        self.units
    }

    /// The units, largest first, so the last ones a thread takes are
    /// small.
    pub(super) fn units_by_weight(&self) -> Vec<usize> {
        let mut weights = vec![0; self.units];
        for (unit, weight) in self.function_unit.iter().zip(&self.weights) {
            if let Some(unit) = unit {
                weights[*unit] += weight;
            }
        }
        for (unit, copies) in self.function_copies.iter().enumerate() {
            weights[unit] += copies.iter().map(|copy| self.weights[*copy]).sum::<usize>();
        }
        let mut order = (0..self.units).collect::<Vec<_>>();
        order.sort_by_key(|unit| (std::cmp::Reverse(weights[*unit]), *unit));
        order
    }

    pub(super) fn function(&self, unit: usize, function: usize) -> Placement {
        placement(
            self.function_unit[function],
            self.function_shared[function],
            self.function_copies[unit].contains(&function),
            unit,
        )
    }

    pub(super) fn data(&self, unit: usize, data: usize) -> Placement {
        placement(
            self.data_unit[data],
            self.data_shared[data],
            self.data_copies[unit].contains(&data),
            unit,
        )
    }
}

/// How many units a module of `weight` recorded instructions splits
/// into. It depends on the module alone, never on the host's cores.
pub(super) fn unit_count(weight: usize) -> usize {
    if weight < SPLIT_WEIGHT {
        1
    } else {
        weight.div_ceil(UNIT_WEIGHT).clamp(2, MAX_UNITS)
    }
}

fn placement(owner: Option<usize>, shared: bool, copied: bool, unit: usize) -> Placement {
    match owner {
        Some(owner) if owner == unit && shared => Placement::Shared,
        Some(owner) if owner == unit => Placement::Private,
        Some(_) if copied => Placement::Copy,
        _ => Placement::Elsewhere,
    }
}

/// A function's recorded instructions, which stand for its LLVM work.
/// The functions `body` calls or takes the address of. The emitter imports
/// every function of the module into every body, so the imports alone
/// would tie each unit to all the others.
fn called_functions(body: &super::FunctionIr) -> BTreeSet<usize> {
    body.insts
        .iter()
        .filter_map(|inst| match &inst.op {
            super::Op::Call(callee, _) | super::Op::FuncAddr(callee) => {
                Some(body.func_refs[callee.0 as usize].0 as usize)
            }
            _ => None,
        })
        .collect()
}

fn function_weight(decl: &super::FunctionDecl) -> usize {
    decl.body.as_ref().map_or(0, |body| body.insts.len())
}
