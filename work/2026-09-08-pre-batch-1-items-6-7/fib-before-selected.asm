<_aura_fn_fib>:
100004000:     	pacibsp
100004004:     	stp	x29, x30, [sp, #-0x10]!
100004008:     	mov	x29, sp
10000400c:     	stp	x24, x28, [sp, #-0x10]!
100004010:     	stp	x19, x23, [sp, #-0x10]!
100004014:     	mov	x19, x0
100004018:     	mov	x0, #0x4                ; =4
10000401c:     	mov	x1, #0x1                ; =1
100004020:     	adrp	x2, 0x100120000 <_writev+0x100120000>
100004024:     	add	x2, x2, #0x483
100004028:     	mov	x3, #0x4a               ; =74
10000402c:     	adrp	x4, 0x100120000 <_writev+0x100120000>
100004030:     	add	x4, x4, #0x480
100004034:     	mov	x5, #0x3                ; =3
100004038:     	adrp	x6, 0x100052000 <__ZN99_$LT$aura_compiler..runtime_value..LightweightTaskContextGuard$u20$as$u20$core..ops..drop..Drop$GT$4drop17h75f4b613d26e523eE+0x190>
10000403c:     	add	x6, x6, #0xa7c
100004040:     	blr	x6
100004044:     	mov	x28, x19
100004048:     	cmp	x28, #0x2
10000404c:     	b.lt	0x100004138 <_aura_fn_fib+0x138>
100004050:     	mov	x10, #0x1               ; =1
100004054:     	subs	x0, x28, x10
100004058:     	cset	x11, vs
10000405c:     	uxtb	w10, w11
100004060:     	cbnz	x10, 0x10000410c <_aura_fn_fib+0x10c>
100004064:     	bl	0x100004000 <_aura_fn_fib>
100004068:     	mov	x24, x0
10000406c:     	mov	x11, #0x2               ; =2
100004070:     	subs	x0, x28, x11
100004074:     	cset	x12, vs
100004078:     	uxtb	w11, w12
10000407c:     	cbnz	x11, 0x1000040e0 <_aura_fn_fib+0xe0>
100004080:     	bl	0x100004000 <_aura_fn_fib>
100004084:     	mov	x3, x24
100004088:     	adds	x23, x3, x0
10000408c:     	cset	x13, vs
100004090:     	uxtb	w12, w13
100004094:     	cbnz	x12, 0x1000040b8 <_aura_fn_fib+0xb8>
100004098:     	adrp	x13, 0x100053000 <_aura_direct_enter_call_with_frame+0x584>
10000409c:     	add	x13, x13, #0x13c
1000040a0:     	blr	x13
1000040a4:     	mov	x0, x23
1000040a8:     	ldp	x19, x23, [sp], #0x10
1000040ac:     	ldp	x24, x28, [sp], #0x10
1000040b0:     	ldp	x29, x30, [sp], #0x10
1000040b4:     	retab
1000040b8:     	mov	x1, #0x0                ; =0
1000040bc:     	mov	x4, #0x7                ; =7
1000040c0:     	mov	x5, #0xc                ; =12
1000040c4:     	adrp	x15, 0x100053000 <_aura_direct_enter_call_with_frame+0x584>
1000040c8:     	add	x15, x15, #0x55c
1000040cc:     	mov	x2, x3
1000040d0:     	mov	x3, x0
1000040d4:     	mov	x0, x1
1000040d8:     	blr	x15
1000040dc:     	udf	#0xc11f
1000040e0:     	mov	x0, #0x0                ; =0
1000040e4:     	mov	x1, #0x1                ; =1
1000040e8:     	mov	x3, #0x2                ; =2
1000040ec:     	mov	x4, #0x7                ; =7
1000040f0:     	mov	x5, #0x1d               ; =29
1000040f4:     	adrp	x6, 0x100053000 <_aura_direct_enter_call_with_frame+0x584>
1000040f8:     	add	x6, x6, #0x55c
1000040fc:     	mov	x2, x28
100004100:     	mov	x19, x28
100004104:     	blr	x6
100004108:     	udf	#0xc11f
10000410c:     	mov	x19, x28
100004110:     	mov	x0, #0x0                ; =0
100004114:     	mov	x3, #0x1                ; =1
100004118:     	mov	x4, #0x7                ; =7
10000411c:     	mov	x5, #0x10               ; =16
100004120:     	adrp	x6, 0x100053000 <_aura_direct_enter_call_with_frame+0x584>
100004124:     	add	x6, x6, #0x55c
100004128:     	mov	x2, x19
10000412c:     	mov	x1, x3
100004130:     	blr	x6
100004134:     	udf	#0xc11f
100004138:     	mov	x19, x28
10000413c:     	adrp	x5, 0x100053000 <_aura_direct_enter_call_with_frame+0x584>
100004140:     	add	x5, x5, #0x13c
100004144:     	blr	x5
100004148:     	mov	x0, x19
10000414c:     	ldp	x19, x23, [sp], #0x10
100004150:     	ldp	x24, x28, [sp], #0x10
100004154:     	ldp	x29, x30, [sp], #0x10
100004158:     	retab

<_aura_direct_enter_call_with_frame>:
100052a7c:     	sub	sp, sp, #0x190
100052a80:     	stp	x26, x25, [sp, #0x140]
100052a84:     	stp	x24, x23, [sp, #0x150]
100052a88:     	stp	x22, x21, [sp, #0x160]
100052a8c:     	stp	x20, x19, [sp, #0x170]
100052a90:     	stp	x29, x30, [sp, #0x180]
100052a94:     	add	x29, sp, #0x180
100052a98:     	cbz	x4, 0x100052f28 <_aura_direct_enter_call_with_frame+0x4ac>
100052a9c:     	mov	x19, x5
100052aa0:     	mov	x24, x4
100052aa4:     	mov	x22, x3
100052aa8:     	mov	x23, x2
100052aac:     	mov	x20, x1
100052ab0:     	mov	x21, x0
100052ab4:     	add	x8, sp, #0x70
100052ab8:     	mov	x0, x4
100052abc:     	mov	x1, x5
100052ac0:     	bl	0x100112ad4 <__RNvNtNtCsl8K0bEFm1U0_4core3str8converts9from_utf8>
100052ac4:     	ldr	x8, [sp, #0x70]
100052ac8:     	cmp	x8, #0x1
100052acc:     	b.eq	0x100052f28 <_aura_direct_enter_call_with_frame+0x4ac>
100052ad0:     	stp	xzr, x24, [sp]
100052ad4:     	str	x19, [sp, #0x10]
100052ad8:     	cbz	x23, 0x100052b04 <_aura_direct_enter_call_with_frame+0x88>
100052adc:     	cbz	x22, 0x100052b04 <_aura_direct_enter_call_with_frame+0x88>
100052ae0:     	add	x8, sp, #0x70
100052ae4:     	mov	x0, x23
100052ae8:     	mov	x1, x22
100052aec:     	bl	0x100112ad4 <__RNvNtNtCsl8K0bEFm1U0_4core3str8converts9from_utf8>
100052af0:     	ldr	x8, [sp, #0x70]
100052af4:     	cmp	x8, #0x1
100052af8:     	b.eq	0x100052f28 <_aura_direct_enter_call_with_frame+0x4ac>
100052afc:     	mov	x8, #0x0                ; =0
100052b00:     	b	0x100052b2c <_aura_direct_enter_call_with_frame+0xb0>
100052b04:     	adrp	x8, 0x100184000 <__ZN13aura_compiler13runtime_value17RUNTIME_SCHEDULER17h8c2e093c103c3bb1E>
100052b08:     	add	x8, x8, #0x40
100052b0c:     	ldapr	x8, [x8]
100052b10:     	cbz	x8, 0x100052b1c <_aura_direct_enter_call_with_frame+0xa0>
100052b14:     	mov	w8, #0x2                ; =2
100052b18:     	b	0x100052b28 <_aura_direct_enter_call_with_frame+0xac>
100052b1c:     	adrp	x9, 0x100184000 <__ZN13aura_compiler13runtime_value17RUNTIME_SCHEDULER17h8c2e093c103c3bb1E>
100052b20:     	add	x9, x9, #0x18
100052b24:     	ldp	x23, x22, [x9]
100052b28:     	ldr	x24, [sp, #0x8]
100052b2c:     	stp	xzr, x24, [sp, #0x18]
100052b30:     	bic	x9, x21, x21, asr #63
100052b34:     	stp	x19, x8, [sp, #0x28]
100052b38:     	bic	x8, x20, x20, asr #63
100052b3c:     	add	x10, x8, #0x1
100052b40:     	stp	x23, x22, [sp, #0x38]
100052b44:     	stp	x9, x8, [sp, #0x48]
100052b48:     	stp	x9, x10, [sp, #0x58]
100052b4c:     	bl	0x100018fec <__ZN13aura_compiler14native_runtime23direct_task_runtime_key17h9edc8b91ee6a1b3dE>
100052b50:     	str	x0, [sp, #0x68]
100052b54:     	ldur	q0, [sp, #0x38]
100052b58:     	ldur	q1, [sp, #0x48]
100052b5c:     	stp	q0, q1, [sp, #0x90]
100052b60:     	ldur	q0, [sp, #0x58]
100052b64:     	str	q0, [sp, #0xb0]
100052b68:     	ldur	q0, [sp, #0x28]
100052b6c:     	ldur	q1, [sp, #0x18]
100052b70:     	stp	q1, q0, [sp, #0x70]
100052b74:     	add	x8, sp, #0x68
100052b78:     	str	x8, [sp, #0xc0]
100052b7c:     	adrp	x0, 0x100185000 <__RNvNvMs0_NtNtNtCsg55jX0GwzBC_3std12backtrace_rs9symbolize5gimliNtB7_5Cache11with_global14MAPPINGS_CACHE+0x750>
100052b80:     	add	x0, x0, #0x2a0
100052b84:     	ldr	x8, [x0]
100052b88:     	blr	x8
100052b8c:     	mov	x19, x0
100052b90:     	ldrb	w8, [x0, #0x20]
100052b94:     	cmp	w8, #0x1
100052b98:     	b.eq	0x100052c28 <_aura_direct_enter_call_with_frame+0x1ac>
100052b9c:     	cmp	w8, #0x2
100052ba0:     	b.ne	0x100052c10 <_aura_direct_enter_call_with_frame+0x194>
100052ba4:     	ldr	x8, [sp, #0x70]
100052ba8:     	cbz	x8, 0x100052bd0 <_aura_direct_enter_call_with_frame+0x154>
100052bac:     	ldr	x8, [sp, #0x78]
100052bb0:     	mov	x9, #-0x1               ; =-1
100052bb4:     	ldaddl	x9, x8, [x8]
100052bb8:     	cmp	x8, #0x1
100052bbc:     	b.ne	0x100052bd0 <_aura_direct_enter_call_with_frame+0x154>
100052bc0:     	add	x8, sp, #0x70
100052bc4:     	dmb	ishld
100052bc8:     	orr	x0, x8, #0x8
100052bcc:     	bl	0x1000450bc <__ZN5alloc4sync16Arc$LT$T$C$A$GT$9drop_slow17h55fc2e6f8491321bE>
100052bd0:     	ldr	x8, [sp, #0x88]
100052bd4:     	cmp	x8, #0x2
100052bd8:     	b.eq	0x100052c04 <_aura_direct_enter_call_with_frame+0x188>
100052bdc:     	cbz	x8, 0x100052c04 <_aura_direct_enter_call_with_frame+0x188>
100052be0:     	ldr	x8, [sp, #0x90]
100052be4:     	mov	x9, #-0x1               ; =-1
100052be8:     	ldaddl	x9, x8, [x8]
100052bec:     	cmp	x8, #0x1
100052bf0:     	b.ne	0x100052c04 <_aura_direct_enter_call_with_frame+0x188>
100052bf4:     	add	x8, sp, #0x70
100052bf8:     	dmb	ishld
100052bfc:     	add	x0, x8, #0x20
100052c00:     	bl	0x1000450bc <__ZN5alloc4sync16Arc$LT$T$C$A$GT$9drop_slow17h55fc2e6f8491321bE>
100052c04:     	adrp	x0, 0x100150000 <_writev+0x100150000>
100052c08:     	add	x0, x0, #0xe18
100052c0c:     	bl	0x10011f59c <__RNvNtNtCsg55jX0GwzBC_3std6thread5local18panic_access_error>
100052c10:     	adrp	x1, 0x10002b000 <__ZN179_$LT$aura_compiler..sema.._..$LT$impl$u20$serde_core..de..Deserialize$u20$for$u20$aura_compiler..sema..Type$GT$..deserialize..__FieldVisitor$u20$as$u20$serde_core..de..Visitor$GT$9visit_str17h9a766b955c2637c6E+0x1ac>
100052c14:     	add	x1, x1, #0x6b4
100052c18:     	mov	x0, x19
100052c1c:     	bl	0x1000ff604 <__RNvNtNtNtNtCsg55jX0GwzBC_3std3sys12thread_local11destructors4list8register>
100052c20:     	mov	w8, #0x1                ; =1
100052c24:     	strb	w8, [x19, #0x20]
100052c28:     	ldr	x8, [x19]
100052c2c:     	cbnz	x8, 0x100052f18 <_aura_direct_enter_call_with_frame+0x49c>
100052c30:     	mov	x8, #-0x1               ; =-1
100052c34:     	str	x8, [x19]
100052c38:     	mov	x8, x19
100052c3c:     	ldr	x10, [x8, #0x8]!
100052c40:     	ldr	x9, [sp, #0x68]
100052c44:     	cbz	x10, 0x100052ca8 <_aura_direct_enter_call_with_frame+0x22c>
100052c48:     	ldr	x11, [x19, #0x10]
100052c4c:     	ldrh	w14, [x10, #0xba]
100052c50:     	add	x13, x10, #0x58
100052c54:     	lsl	x15, x14, #3
100052c58:     	mov	x12, #-0x1              ; =-1
100052c5c:     	cbz	x15, 0x100052c90 <_aura_direct_enter_call_with_frame+0x214>
100052c60:     	ldur	x16, [x13, #-0x50]
100052c64:     	cmp	x9, x16
100052c68:     	cset	w16, hi
100052c6c:     	csinv	w16, w16, wzr, hs
100052c70:     	add	x13, x13, #0x8
100052c74:     	add	x12, x12, #0x1
100052c78:     	sub	x15, x15, #0x8
100052c7c:     	and	w16, w16, #0xff
100052c80:     	cmp	w16, #0x1
100052c84:     	b.eq	0x100052c5c <_aura_direct_enter_call_with_frame+0x1e0>
100052c88:     	cbz	w16, 0x100052ce0 <_aura_direct_enter_call_with_frame+0x264>
100052c8c:     	b	0x100052c94 <_aura_direct_enter_call_with_frame+0x218>
100052c90:     	mov	x12, x14
100052c94:     	cbz	x11, 0x100052f08 <_aura_direct_enter_call_with_frame+0x48c>
100052c98:     	add	x10, x10, x12, lsl #3
100052c9c:     	ldr	x10, [x10, #0xc0]
100052ca0:     	sub	x11, x11, #0x1
100052ca4:     	b	0x100052c4c <_aura_direct_enter_call_with_frame+0x1d0>
100052ca8:     	stp	x8, x9, [x29, #-0x90]
100052cac:     	stur	xzr, [x29, #-0x80]
100052cb0:     	mov	w0, #0x0                ; =0
100052cb4:     	mov	x1, #0x0                ; =0
100052cb8:     	mov	x2, #0x0                ; =0
100052cbc:     	bl	0x10001bee4 <__ZN13aura_compiler14native_runtime31boxed_direct_task_runtime_state17h915972e656d02e3cE>
100052cc0:     	mov	x2, x0
100052cc4:     	sub	x0, x29, #0xb0
100052cc8:     	sub	x1, x29, #0x90
100052ccc:     	bl	0x10003e68c <__ZN5alloc11collections5btree3map5entry28VacantEntry$LT$K$C$V$C$A$GT$12insert_entry17h506ed4e48a322b10E>
100052cd0:     	ldur	x8, [x29, #-0xb0]
100052cd4:     	ldur	x9, [x29, #-0xa0]
100052cd8:     	add	x8, x8, x9, lsl #3
100052cdc:     	add	x13, x8, #0x60
100052ce0:     	ldr	x24, [x13]
100052ce4:     	ldr	x8, [x24, #0x170]
100052ce8:     	cmp	x8, #0xff
100052cec:     	b.hi	0x100052f2c <_aura_direct_enter_call_with_frame+0x4b0>
100052cf0:     	ldp	x9, x20, [x24, #0xf0]
100052cf4:     	ldp	x21, x25, [x24, #0x100]
100052cf8:     	ldr	x22, [x24, #0x110]
100052cfc:     	mov	x10, #-0x7fffffffffffffff ; =-9223372036854775807
100052d00:     	str	x10, [x24, #0xf0]
100052d04:     	mov	x11, #-0x8000000000000000 ; =-9223372036854775808
100052d08:     	cmp	x9, x11
100052d0c:     	b.eq	0x100052d24 <_aura_direct_enter_call_with_frame+0x2a8>
100052d10:     	cmp	x9, x10
100052d14:     	b.ne	0x100052fa8 <_aura_direct_enter_call_with_frame+0x52c>
100052d18:     	mov	x25, #0x0               ; =0
100052d1c:     	mov	x20, #0x0               ; =0
100052d20:     	mov	w21, #0x8               ; =8
100052d24:     	add	x9, x8, #0x1
100052d28:     	mov	x22, x24
100052d2c:     	ldr	x8, [x22, #0x58]!
100052d30:     	str	x9, [x24, #0x170]
100052d34:     	mov	w9, #0x2                ; =2
100052d38:     	str	x9, [x22]
100052d3c:     	sub	x9, x8, #0x2
100052d40:     	cmp	x8, #0x1
100052d44:     	csinc	x9, x9, xzr, hi
100052d48:     	cbz	x9, 0x100052de8 <_aura_direct_enter_call_with_frame+0x36c>
100052d4c:     	cmp	x9, #0x1
100052d50:     	b.ne	0x100052e14 <_aura_direct_enter_call_with_frame+0x398>
100052d54:     	stur	x8, [x29, #-0x90]
100052d58:     	ldur	q0, [x22, #0x8]
100052d5c:     	stur	q0, [x29, #-0x88]
100052d60:     	ldur	x8, [x22, #0x18]
100052d64:     	stur	x8, [x29, #-0x78]
100052d68:     	ldur	q0, [x24, #0x78]
100052d6c:     	ldur	q1, [x24, #0x88]
100052d70:     	stp	q0, q1, [x29, #-0x70]
100052d74:     	ldur	q0, [x24, #0x98]
100052d78:     	stur	q0, [x29, #-0x50]
100052d7c:     	bl	0x100056c94 <__RNvCsfLfy6EI15iL_7___rustc35___rust_no_alloc_shim_is_unstable_v2>
100052d80:     	mov	w0, #0x140              ; =320
100052d84:     	mov	w1, #0x8                ; =8
100052d88:     	bl	0x100056c84 <__RNvCsfLfy6EI15iL_7___rustc12___rust_alloc>
100052d8c:     	cbz	x0, 0x10005305c <_aura_direct_enter_call_with_frame+0x5e0>
100052d90:     	mov	x23, x0
100052d94:     	ldp	q0, q1, [x29, #-0x70]
100052d98:     	stp	q0, q1, [x0, #0x20]
100052d9c:     	ldp	q1, q0, [x29, #-0x90]
100052da0:     	stp	q1, q0, [x0]
100052da4:     	ldur	q0, [sp, #0x38]
100052da8:     	ldur	q1, [sp, #0x48]
100052dac:     	ldur	q2, [sp, #0x28]
100052db0:     	ldur	q3, [sp, #0x18]
100052db4:     	stp	q2, q0, [x0, #0x60]
100052db8:     	ldur	q0, [sp, #0x58]
100052dbc:     	stp	q1, q0, [x0, #0x80]
100052dc0:     	ldur	q0, [x29, #-0x50]
100052dc4:     	stp	q0, q3, [x0, #0x40]
100052dc8:     	mov	x0, x22
100052dcc:     	bl	0x100037f60 <__ZN4core3ptr74drop_in_place$LT$aura_compiler..native_runtime..DirectCallFrameStorage$GT$17he81610866b0752daE>
100052dd0:     	mov	w8, #0x4                ; =4
100052dd4:     	dup.2d	v0, x8
100052dd8:     	stur	q0, [x24, #0x58]
100052ddc:     	mov	w8, #0x2                ; =2
100052de0:     	stp	x23, x8, [x24, #0x68]
100052de4:     	b	0x100052e8c <_aura_direct_enter_call_with_frame+0x410>
100052de8:     	mov	x0, x22
100052dec:     	bl	0x100037f60 <__ZN4core3ptr74drop_in_place$LT$aura_compiler..native_runtime..DirectCallFrameStorage$GT$17he81610866b0752daE>
100052df0:     	ldur	q0, [sp, #0x38]
100052df4:     	ldur	q1, [sp, #0x48]
100052df8:     	stp	q0, q1, [x22, #0x20]
100052dfc:     	ldur	q0, [sp, #0x58]
100052e00:     	str	q0, [x22, #0x40]
100052e04:     	ldur	q0, [sp, #0x28]
100052e08:     	ldur	q1, [sp, #0x18]
100052e0c:     	stp	q1, q0, [x22]
100052e10:     	b	0x100052e8c <_aura_direct_enter_call_with_frame+0x410>
100052e14:     	ldur	q0, [x22, #0x8]
100052e18:     	stur	q0, [x29, #-0x90]
100052e1c:     	ldur	x23, [x22, #0x18]
100052e20:     	stur	x23, [x29, #-0x80]
100052e24:     	ldur	x8, [x29, #-0x90]
100052e28:     	cmp	x23, x8
100052e2c:     	b.ne	0x100052e38 <_aura_direct_enter_call_with_frame+0x3bc>
100052e30:     	sub	x0, x29, #0x90
100052e34:     	bl	0x1001192f8 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hedad82ba78b3e9f5E>
100052e38:     	ldur	x8, [x29, #-0x88]
100052e3c:     	mov	w9, #0x50               ; =80
100052e40:     	madd	x8, x23, x9, x8
100052e44:     	ldur	q0, [sp, #0x38]
100052e48:     	ldur	q1, [sp, #0x48]
100052e4c:     	stp	q0, q1, [x8, #0x20]
100052e50:     	ldur	q0, [sp, #0x58]
100052e54:     	str	q0, [x8, #0x40]
100052e58:     	ldur	q0, [sp, #0x28]
100052e5c:     	ldur	q1, [sp, #0x18]
100052e60:     	stp	q1, q0, [x8]
100052e64:     	add	x8, x23, #0x1
100052e68:     	stur	x8, [x29, #-0x80]
100052e6c:     	mov	x0, x22
100052e70:     	bl	0x100037f60 <__ZN4core3ptr74drop_in_place$LT$aura_compiler..native_runtime..DirectCallFrameStorage$GT$17he81610866b0752daE>
100052e74:     	mov	w8, #0x4                ; =4
100052e78:     	str	x8, [x22]
100052e7c:     	ldur	q0, [x29, #-0x90]
100052e80:     	stur	q0, [x22, #0x8]
100052e84:     	ldur	x8, [x29, #-0x80]
100052e88:     	stur	x8, [x22, #0x18]
100052e8c:     	mov	x0, x24
100052e90:     	ldr	x8, [x0, #0xd8]!
100052e94:     	mov	x9, #-0x8000000000000000 ; =-9223372036854775808
100052e98:     	stur	x9, [x29, #-0x60]
100052e9c:     	stp	x20, x21, [x29, #-0x90]
100052ea0:     	stp	x25, x9, [x29, #-0x80]
100052ea4:     	ldr	x20, [x0, #0x10]
100052ea8:     	cmp	x20, x8
100052eac:     	b.ne	0x100052eb4 <_aura_direct_enter_call_with_frame+0x438>
100052eb0:     	bl	0x100119430 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hfd943772789582afE>
100052eb4:     	ldr	x8, [x24, #0xe0]
100052eb8:     	mov	w9, #0x48               ; =72
100052ebc:     	madd	x8, x20, x9, x8
100052ec0:     	ldp	q0, q1, [x29, #-0x70]
100052ec4:     	stp	q0, q1, [x8, #0x20]
100052ec8:     	ldur	x9, [x29, #-0x50]
100052ecc:     	str	x9, [x8, #0x40]
100052ed0:     	ldp	q1, q0, [x29, #-0x90]
100052ed4:     	stp	q1, q0, [x8]
100052ed8:     	add	x8, x20, #0x1
100052edc:     	str	x8, [x24, #0xe8]
100052ee0:     	ldr	x8, [x19]
100052ee4:     	add	x8, x8, #0x1
100052ee8:     	str	x8, [x19]
100052eec:     	ldp	x29, x30, [sp, #0x180]
100052ef0:     	ldp	x20, x19, [sp, #0x170]
100052ef4:     	ldp	x22, x21, [sp, #0x160]
100052ef8:     	ldp	x24, x23, [sp, #0x150]
100052efc:     	ldp	x26, x25, [sp, #0x140]
100052f00:     	add	sp, sp, #0x190
100052f04:     	ret
100052f08:     	stp	x8, x9, [x29, #-0x90]
100052f0c:     	stp	x10, xzr, [x29, #-0x80]
100052f10:     	stur	x12, [x29, #-0x70]
100052f14:     	b	0x100052cb0 <_aura_direct_enter_call_with_frame+0x234>
100052f18:     	adrp	x0, 0x100150000 <_writev+0x100150000>
100052f1c:     	add	x0, x0, #0x8c0
100052f20:     	bl	0x10011fb8c <__RNvNtCsl8K0bEFm1U0_4core4cell22panic_already_borrowed>
100052f24:     	b	0x100053068 <_aura_direct_enter_call_with_frame+0x5ec>
100052f28:     	bl	0x1001186ec <__ZN13aura_compiler14native_runtime32reject_invalid_direct_frame_utf817h809008fa117e25e2E>
100052f2c:     	ldr	x8, [sp, #0x70]
100052f30:     	cbz	x8, 0x100052f58 <_aura_direct_enter_call_with_frame+0x4dc>
100052f34:     	ldr	x8, [sp, #0x78]
100052f38:     	mov	x9, #-0x1               ; =-1
100052f3c:     	ldaddl	x9, x8, [x8]
100052f40:     	cmp	x8, #0x1
100052f44:     	b.ne	0x100052f58 <_aura_direct_enter_call_with_frame+0x4dc>
100052f48:     	add	x8, sp, #0x70
100052f4c:     	dmb	ishld
100052f50:     	orr	x0, x8, #0x8
100052f54:     	bl	0x1000450bc <__ZN5alloc4sync16Arc$LT$T$C$A$GT$9drop_slow17h55fc2e6f8491321bE>
100052f58:     	ldr	x8, [sp, #0x88]
100052f5c:     	cmp	x8, #0x2
100052f60:     	b.eq	0x100052f8c <_aura_direct_enter_call_with_frame+0x510>
100052f64:     	cbz	x8, 0x100052f8c <_aura_direct_enter_call_with_frame+0x510>
100052f68:     	ldr	x8, [sp, #0x90]
100052f6c:     	mov	x9, #-0x1               ; =-1
100052f70:     	ldaddl	x9, x8, [x8]
100052f74:     	cmp	x8, #0x1
100052f78:     	b.ne	0x100052f8c <_aura_direct_enter_call_with_frame+0x510>
100052f7c:     	add	x8, sp, #0x70
100052f80:     	dmb	ishld
100052f84:     	add	x0, x8, #0x20
100052f88:     	bl	0x1000450bc <__ZN5alloc4sync16Arc$LT$T$C$A$GT$9drop_slow17h55fc2e6f8491321bE>
100052f8c:     	ldr	x8, [x19]
100052f90:     	add	x8, x8, #0x1
100052f94:     	str	x8, [x19]
100052f98:     	mov	x2, sp
100052f9c:     	mov	x0, x21
100052fa0:     	mov	x1, x20
100052fa4:     	bl	0x1001186c4 <__ZN13aura_compiler14native_runtime24reject_direct_call_depth17h45153b0a0ca4a581E>
100052fa8:     	cbnz	x9, 0x100052fbc <_aura_direct_enter_call_with_frame+0x540>
100052fac:     	cbnz	x25, 0x100052fd0 <_aura_direct_enter_call_with_frame+0x554>
100052fb0:     	ldr	x8, [sp, #0x70]
100052fb4:     	cbnz	x8, 0x100052fe8 <_aura_direct_enter_call_with_frame+0x56c>
100052fb8:     	b	0x10005300c <_aura_direct_enter_call_with_frame+0x590>
100052fbc:     	lsl	x1, x9, #3
100052fc0:     	mov	x0, x20
100052fc4:     	mov	w2, #0x8                ; =8
100052fc8:     	bl	0x100056c88 <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
100052fcc:     	cbz	x25, 0x100052fb0 <_aura_direct_enter_call_with_frame+0x534>
100052fd0:     	lsl	x1, x25, #4
100052fd4:     	mov	x0, x22
100052fd8:     	mov	w2, #0x8                ; =8
100052fdc:     	bl	0x100056c88 <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
100052fe0:     	ldr	x8, [sp, #0x70]
100052fe4:     	cbz	x8, 0x10005300c <_aura_direct_enter_call_with_frame+0x590>
100052fe8:     	ldr	x8, [sp, #0x78]
100052fec:     	mov	x9, #-0x1               ; =-1
100052ff0:     	ldaddl	x9, x8, [x8]
100052ff4:     	cmp	x8, #0x1
100052ff8:     	b.ne	0x10005300c <_aura_direct_enter_call_with_frame+0x590>
100052ffc:     	add	x8, sp, #0x70
100053000:     	dmb	ishld
100053004:     	orr	x0, x8, #0x8
100053008:     	bl	0x1000450bc <__ZN5alloc4sync16Arc$LT$T$C$A$GT$9drop_slow17h55fc2e6f8491321bE>
10005300c:     	ldr	x8, [sp, #0x88]
100053010:     	cmp	x8, #0x2
100053014:     	b.eq	0x100053040 <_aura_direct_enter_call_with_frame+0x5c4>
100053018:     	cbz	x8, 0x100053040 <_aura_direct_enter_call_with_frame+0x5c4>
10005301c:     	ldr	x8, [sp, #0x90]
100053020:     	mov	x9, #-0x1               ; =-1
100053024:     	ldaddl	x9, x8, [x8]
100053028:     	cmp	x8, #0x1
10005302c:     	b.ne	0x100053040 <_aura_direct_enter_call_with_frame+0x5c4>
100053030:     	add	x8, sp, #0x70
100053034:     	dmb	ishld
100053038:     	add	x0, x8, #0x20
10005303c:     	bl	0x1000450bc <__ZN5alloc4sync16Arc$LT$T$C$A$GT$9drop_slow17h55fc2e6f8491321bE>
100053040:     	ldr	x8, [x19]
100053044:     	add	x8, x8, #0x1
100053048:     	str	x8, [x19]
10005304c:     	adrp	x0, 0x100124000 <_aura_data_145+0x3920>
100053050:     	add	x0, x0, #0x2
100053054:     	mov	w1, #0x48               ; =72
100053058:     	bl	0x100016a10 <__ZN13aura_compiler14native_runtime13runtime_error17h4e09389ff41b4f49E>
10005305c:     	mov	w0, #0x8                ; =8
100053060:     	mov	w1, #0x140              ; =320
100053064:     	bl	0x10011fafc <__RNvNtCs1OjIl8oxbrv_5alloc7raw_vec12handle_error>
100053068:     	brk	#0x1
10005306c:     	mov	x22, x0
100053070:     	add	x0, sp, #0x70
100053074:     	bl	0x100033d00 <__ZN4core3ptr242drop_in_place$LT$aura_compiler..native_runtime..with_direct_task_runtime_state_for_key$LT$$LP$bool$C$bool$RP$$C$aura_compiler..native_runtime..aura_direct_enter_call_with_frame..$u7b$$u7b$closure$u7d$$u7d$$GT$..$u7b$$u7b$closure$u7d$$u7d$$GT$17h73144f6f18bc8b3cE>
100053078:     	mov	x0, x22
10005307c:     	bl	0x10011ffd4 <_writev+0x10011ffd4>
100053080:     	mov	x22, x0
100053084:     	add	x0, sp, #0x70
100053088:     	bl	0x1000380b0 <__ZN4core3ptr74drop_in_place$LT$aura_compiler..native_runtime..DirectRuntimeCallFrame$GT$17h2eb3d5c1b3f8f270E>
10005308c:     	sub	x0, x29, #0x90
100053090:     	bl	0x100039c98 <__ZN4core3ptr97drop_in_place$LT$alloc..vec..Vec$LT$aura_compiler..native_runtime..DirectRuntimeCallFrame$GT$$GT$17hca8b7dd66ff55872E>
100053094:     	b	0x1000530ac <_aura_direct_enter_call_with_frame+0x630>
100053098:     	mov	x22, x0
10005309c:     	sub	x0, x29, #0x90
1000530a0:     	bl	0x1000380b0 <__ZN4core3ptr74drop_in_place$LT$aura_compiler..native_runtime..DirectRuntimeCallFrame$GT$17h2eb3d5c1b3f8f270E>
1000530a4:     	add	x0, sp, #0x70
1000530a8:     	bl	0x1000380b0 <__ZN4core3ptr74drop_in_place$LT$aura_compiler..native_runtime..DirectRuntimeCallFrame$GT$17h2eb3d5c1b3f8f270E>
1000530ac:     	cbz	x20, 0x100053100 <_aura_direct_enter_call_with_frame+0x684>
1000530b0:     	lsl	x1, x20, #3
1000530b4:     	mov	x0, x21
1000530b8:     	mov	w2, #0x8                ; =8
1000530bc:     	bl	0x100056c88 <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
1000530c0:     	ldr	x8, [x19]
1000530c4:     	add	x8, x8, #0x1
1000530c8:     	str	x8, [x19]
1000530cc:     	mov	x0, x22
1000530d0:     	bl	0x10011ffd4 <_writev+0x10011ffd4>
1000530d4:     	mov	x22, x0
1000530d8:     	sub	x0, x29, #0x90
1000530dc:     	bl	0x1000386fc <__ZN4core3ptr75drop_in_place$LT$aura_compiler..native_runtime..DirectReturnedViewFrame$GT$17heb7e7b178af11af4E>
1000530e0:     	ldr	x8, [x19]
1000530e4:     	add	x8, x8, #0x1
1000530e8:     	str	x8, [x19]
1000530ec:     	mov	x0, x22
1000530f0:     	bl	0x10011ffd4 <_writev+0x10011ffd4>
1000530f4:     	mov	x22, x0
1000530f8:     	add	x0, sp, #0x70
1000530fc:     	bl	0x100031258 <__ZN4core3ptr114drop_in_place$LT$aura_compiler..native_runtime..aura_direct_enter_call_with_frame..$u7b$$u7b$closure$u7d$$u7d$$GT$17h06397572cacb4732E>
100053100:     	ldr	x8, [x19]
100053104:     	add	x8, x8, #0x1
100053108:     	str	x8, [x19]
10005310c:     	mov	x0, x22
100053110:     	bl	0x10011ffd4 <_writev+0x10011ffd4>
100053114:     	mov	x22, x0
100053118:     	add	x0, sp, #0x18
10005311c:     	bl	0x100031258 <__ZN4core3ptr114drop_in_place$LT$aura_compiler..native_runtime..aura_direct_enter_call_with_frame..$u7b$$u7b$closure$u7d$$u7d$$GT$17h06397572cacb4732E>
100053120:     	mov	x0, x22
100053124:     	bl	0x10011ffd4 <_writev+0x10011ffd4>
100053128:     	mov	x22, x0
10005312c:     	add	x0, sp, #0x70
100053130:     	bl	0x100031258 <__ZN4core3ptr114drop_in_place$LT$aura_compiler..native_runtime..aura_direct_enter_call_with_frame..$u7b$$u7b$closure$u7d$$u7d$$GT$17h06397572cacb4732E>
100053134:     	mov	x0, x22
100053138:     	bl	0x10011ffd4 <_writev+0x10011ffd4>

<_aura_direct_exit_call>:
10005313c:     	sub	sp, sp, #0x140
100053140:     	stp	x28, x27, [sp, #0xe0]
100053144:     	stp	x26, x25, [sp, #0xf0]
100053148:     	stp	x24, x23, [sp, #0x100]
10005314c:     	stp	x22, x21, [sp, #0x110]
100053150:     	stp	x20, x19, [sp, #0x120]
100053154:     	stp	x29, x30, [sp, #0x130]
100053158:     	add	x29, sp, #0x130
10005315c:     	bl	0x100018fec <__ZN13aura_compiler14native_runtime23direct_task_runtime_key17h9edc8b91ee6a1b3dE>
100053160:     	mov	x20, x0
100053164:     	adrp	x0, 0x100185000 <__RNvNvMs0_NtNtNtCsg55jX0GwzBC_3std12backtrace_rs9symbolize5gimliNtB7_5Cache11with_global14MAPPINGS_CACHE+0x750>
100053168:     	add	x0, x0, #0x2a0
10005316c:     	ldr	x8, [x0]
100053170:     	blr	x8
100053174:     	mov	x19, x0
100053178:     	ldrb	w8, [x0, #0x20]
10005317c:     	cmp	w8, #0x1
100053180:     	b.eq	0x1000531b0 <_aura_direct_exit_call+0x74>
100053184:     	cmp	w8, #0x2
100053188:     	b.ne	0x100053198 <_aura_direct_exit_call+0x5c>
10005318c:     	adrp	x0, 0x100150000 <_writev+0x100150000>
100053190:     	add	x0, x0, #0xe18
100053194:     	bl	0x10011f59c <__RNvNtNtCsg55jX0GwzBC_3std6thread5local18panic_access_error>
100053198:     	adrp	x1, 0x10002b000 <__ZN179_$LT$aura_compiler..sema.._..$LT$impl$u20$serde_core..de..Deserialize$u20$for$u20$aura_compiler..sema..Type$GT$..deserialize..__FieldVisitor$u20$as$u20$serde_core..de..Visitor$GT$9visit_str17h9a766b955c2637c6E+0x1ac>
10005319c:     	add	x1, x1, #0x6b4
1000531a0:     	mov	x0, x19
1000531a4:     	bl	0x1000ff604 <__RNvNtNtNtNtCsg55jX0GwzBC_3std3sys12thread_local11destructors4list8register>
1000531a8:     	mov	w8, #0x1                ; =1
1000531ac:     	strb	w8, [x19, #0x20]
1000531b0:     	ldr	x8, [x19]
1000531b4:     	cbnz	x8, 0x100053514 <_aura_direct_exit_call+0x3d8>
1000531b8:     	mov	x8, #-0x1               ; =-1
1000531bc:     	str	x8, [x19]
1000531c0:     	mov	x8, x19
1000531c4:     	ldr	x9, [x8, #0x8]!
1000531c8:     	cbz	x9, 0x10005322c <_aura_direct_exit_call+0xf0>
1000531cc:     	ldr	x10, [x19, #0x10]
1000531d0:     	ldrh	w13, [x9, #0xba]
1000531d4:     	add	x12, x9, #0x58
1000531d8:     	lsl	x14, x13, #3
1000531dc:     	mov	x11, #-0x1              ; =-1
1000531e0:     	cbz	x14, 0x100053214 <_aura_direct_exit_call+0xd8>
1000531e4:     	ldur	x15, [x12, #-0x50]
1000531e8:     	cmp	x20, x15
1000531ec:     	cset	w15, hi
1000531f0:     	csinv	w15, w15, wzr, hs
1000531f4:     	add	x12, x12, #0x8
1000531f8:     	add	x11, x11, #0x1
1000531fc:     	sub	x14, x14, #0x8
100053200:     	and	w15, w15, #0xff
100053204:     	cmp	w15, #0x1
100053208:     	b.eq	0x1000531e0 <_aura_direct_exit_call+0xa4>
10005320c:     	cbz	w15, 0x100053264 <_aura_direct_exit_call+0x128>
100053210:     	b	0x100053218 <_aura_direct_exit_call+0xdc>
100053214:     	mov	x11, x13
100053218:     	cbz	x10, 0x100053504 <_aura_direct_exit_call+0x3c8>
10005321c:     	add	x9, x9, x11, lsl #3
100053220:     	ldr	x9, [x9, #0xc0]
100053224:     	sub	x10, x10, #0x1
100053228:     	b	0x1000531d0 <_aura_direct_exit_call+0x94>
10005322c:     	stp	x8, x20, [sp]
100053230:     	str	xzr, [sp, #0x10]
100053234:     	mov	w0, #0x0                ; =0
100053238:     	mov	x1, #0x0                ; =0
10005323c:     	mov	x2, #0x0                ; =0
100053240:     	bl	0x10001bee4 <__ZN13aura_compiler14native_runtime31boxed_direct_task_runtime_state17h915972e656d02e3cE>
100053244:     	mov	x2, x0
100053248:     	add	x0, sp, #0x50
10005324c:     	mov	x1, sp
100053250:     	bl	0x10003e68c <__ZN5alloc11collections5btree3map5entry28VacantEntry$LT$K$C$V$C$A$GT$12insert_entry17h506ed4e48a322b10E>
100053254:     	ldr	x8, [sp, #0x50]
100053258:     	ldr	x9, [sp, #0x60]
10005325c:     	add	x8, x8, x9, lsl #3
100053260:     	add	x12, x8, #0x60
100053264:     	ldr	x22, [x12]
100053268:     	ldr	x8, [x22, #0x170]
10005326c:     	cbz	x8, 0x1000534c4 <_aura_direct_exit_call+0x388>
100053270:     	sub	x8, x8, #0x1
100053274:     	str	x8, [x22, #0x170]
100053278:     	mov	x21, x22
10005327c:     	ldp	x23, x24, [x21, #0x58]!
100053280:     	ldp	x20, x8, [x21, #0x10]
100053284:     	mov	w9, #0x2                ; =2
100053288:     	str	x9, [x21]
10005328c:     	sub	x9, x23, #0x2
100053290:     	cmp	x23, #0x1
100053294:     	csinc	x9, x9, xzr, hi
100053298:     	cbz	x9, 0x100053438 <_aura_direct_exit_call+0x2fc>
10005329c:     	cmp	x9, #0x1
1000532a0:     	b.ne	0x1000532c8 <_aura_direct_exit_call+0x18c>
1000532a4:     	ldp	q0, q1, [x21, #0x20]
1000532a8:     	stp	q0, q1, [sp, #0x20]
1000532ac:     	ldur	q0, [x21, #0x40]
1000532b0:     	str	q0, [sp, #0x40]
1000532b4:     	stp	x23, x24, [sp]
1000532b8:     	stp	x20, x8, [sp, #0x10]
1000532bc:     	cmp	x23, #0x2
1000532c0:     	b.ne	0x100053398 <_aura_direct_exit_call+0x25c>
1000532c4:     	b	0x100053438 <_aura_direct_exit_call+0x2fc>
1000532c8:     	cbz	x8, 0x100053324 <_aura_direct_exit_call+0x1e8>
1000532cc:     	subs	x25, x8, #0x1
1000532d0:     	mov	w9, #0x50               ; =80
1000532d4:     	madd	x9, x25, x9, x20
1000532d8:     	ldr	x23, [x9]
1000532dc:     	ldur	q0, [x9, #0x18]
1000532e0:     	ldur	q1, [x9, #0x28]
1000532e4:     	ldur	q2, [x9, #0x38]
1000532e8:     	stp	q1, q2, [sp, #0x70]
1000532ec:     	ldr	x10, [x9, #0x48]
1000532f0:     	str	x10, [sp, #0x90]
1000532f4:     	ldur	q1, [x9, #0x8]
1000532f8:     	stp	q1, q0, [sp, #0x50]
1000532fc:     	b.eq	0x100053330 <_aura_direct_exit_call+0x1f4>
100053300:     	cmp	x8, #0x2
100053304:     	b.ne	0x1000533f8 <_aura_direct_exit_call+0x2bc>
100053308:     	ldp	x25, x26, [x20]
10005330c:     	ldp	x27, x28, [x20, #0x10]
100053310:     	ldp	q0, q1, [x20, #0x20]
100053314:     	stp	q0, q1, [x29, #-0x90]
100053318:     	ldr	q0, [x20, #0x40]
10005331c:     	stur	q0, [x29, #-0x70]
100053320:     	b	0x100053334 <_aura_direct_exit_call+0x1f8>
100053324:     	mov	w23, #0x2               ; =2
100053328:     	mov	w25, #0x2               ; =2
10005332c:     	b	0x100053334 <_aura_direct_exit_call+0x1f8>
100053330:     	mov	w25, #0x2               ; =2
100053334:     	mov	x0, x21
100053338:     	bl	0x100037f60 <__ZN4core3ptr74drop_in_place$LT$aura_compiler..native_runtime..DirectCallFrameStorage$GT$17he81610866b0752daE>
10005333c:     	ldp	q0, q1, [x29, #-0x90]
100053340:     	stp	q0, q1, [x21, #0x20]
100053344:     	ldur	q0, [x29, #-0x70]
100053348:     	stur	q0, [x21, #0x40]
10005334c:     	ldp	q1, q0, [sp, #0x50]
100053350:     	stur	q0, [sp, #0x18]
100053354:     	ldp	q0, q2, [sp, #0x70]
100053358:     	stur	q0, [sp, #0x28]
10005335c:     	stur	q2, [sp, #0x38]
100053360:     	stp	x25, x26, [x22, #0x58]
100053364:     	stp	x27, x28, [x22, #0x68]
100053368:     	str	x23, [sp]
10005336c:     	ldr	x8, [sp, #0x90]
100053370:     	str	x8, [sp, #0x48]
100053374:     	stur	q1, [sp, #0x8]
100053378:     	cbz	x24, 0x100053390 <_aura_direct_exit_call+0x254>
10005337c:     	add	x8, x24, x24, lsl #2
100053380:     	lsl	x1, x8, #4
100053384:     	mov	x0, x20
100053388:     	mov	w2, #0x8                ; =8
10005338c:     	bl	0x100056c88 <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
100053390:     	cmp	x23, #0x2
100053394:     	b.eq	0x100053438 <_aura_direct_exit_call+0x2fc>
100053398:     	cbz	x23, 0x1000533c0 <_aura_direct_exit_call+0x284>
10005339c:     	ldr	x8, [sp, #0x8]
1000533a0:     	mov	x9, #-0x1               ; =-1
1000533a4:     	ldaddl	x9, x8, [x8]
1000533a8:     	cmp	x8, #0x1
1000533ac:     	b.ne	0x1000533c0 <_aura_direct_exit_call+0x284>
1000533b0:     	mov	x8, sp
1000533b4:     	dmb	ishld
1000533b8:     	add	x0, x8, #0x8
1000533bc:     	bl	0x1000450bc <__ZN5alloc4sync16Arc$LT$T$C$A$GT$9drop_slow17h55fc2e6f8491321bE>
1000533c0:     	ldr	x8, [sp, #0x18]
1000533c4:     	cmp	x8, #0x2
1000533c8:     	b.eq	0x100053438 <_aura_direct_exit_call+0x2fc>
1000533cc:     	cbz	x8, 0x100053438 <_aura_direct_exit_call+0x2fc>
1000533d0:     	ldr	x8, [sp, #0x20]
1000533d4:     	mov	x9, #-0x1               ; =-1
1000533d8:     	ldaddl	x9, x8, [x8]
1000533dc:     	cmp	x8, #0x1
1000533e0:     	b.ne	0x100053438 <_aura_direct_exit_call+0x2fc>
1000533e4:     	mov	x8, sp
1000533e8:     	dmb	ishld
1000533ec:     	add	x0, x8, #0x20
1000533f0:     	bl	0x1000450bc <__ZN5alloc4sync16Arc$LT$T$C$A$GT$9drop_slow17h55fc2e6f8491321bE>
1000533f4:     	b	0x100053438 <_aura_direct_exit_call+0x2fc>
1000533f8:     	mov	x0, x21
1000533fc:     	bl	0x100037f60 <__ZN4core3ptr74drop_in_place$LT$aura_compiler..native_runtime..DirectCallFrameStorage$GT$17he81610866b0752daE>
100053400:     	mov	w8, #0x4                ; =4
100053404:     	str	x8, [x22, #0x58]
100053408:     	str	x25, [x22, #0x70]
10005340c:     	str	x23, [sp]
100053410:     	ldp	q1, q0, [sp, #0x50]
100053414:     	stur	q0, [sp, #0x18]
100053418:     	ldp	q0, q2, [sp, #0x70]
10005341c:     	stur	q0, [sp, #0x28]
100053420:     	stur	q2, [sp, #0x38]
100053424:     	ldr	x8, [sp, #0x90]
100053428:     	str	x8, [sp, #0x48]
10005342c:     	stur	q1, [sp, #0x8]
100053430:     	cmp	x23, #0x2
100053434:     	b.ne	0x100053398 <_aura_direct_exit_call+0x25c>
100053438:     	ldr	x8, [x22, #0xe8]
10005343c:     	cbz	x8, 0x1000534c4 <_aura_direct_exit_call+0x388>
100053440:     	sub	x25, x8, #0x1
100053444:     	str	x25, [x22, #0xe8]
100053448:     	ldr	x8, [x22, #0xe0]
10005344c:     	mov	w9, #0x48               ; =72
100053450:     	madd	x23, x25, x9, x8
100053454:     	ldp	x26, x22, [x23]
100053458:     	ldp	x20, x21, [x23, #0x18]
10005345c:     	ldp	x24, x1, [x23, #0x28]
100053460:     	tst	x1, #0x7fffffffffffffff
100053464:     	b.ne	0x1000534f0 <_aura_direct_exit_call+0x3b4>
100053468:     	cbz	x26, 0x10005347c <_aura_direct_exit_call+0x340>
10005346c:     	lsl	x1, x26, #3
100053470:     	mov	x0, x22
100053474:     	mov	w2, #0x8                ; =8
100053478:     	bl	0x100056c88 <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10005347c:     	cbz	x25, 0x1000534ac <_aura_direct_exit_call+0x370>
100053480:     	ldur	x1, [x23, #-0x18]
100053484:     	mov	x8, #-0x8000000000000000 ; =-9223372036854775808
100053488:     	cmp	x1, x8
10005348c:     	ccmp	x1, #0x0, #0x4, ne
100053490:     	b.eq	0x1000534a0 <_aura_direct_exit_call+0x364>
100053494:     	ldur	x0, [x23, #-0x10]
100053498:     	mov	w2, #0x1                ; =1
10005349c:     	bl	0x100056c88 <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
1000534a0:     	stp	x20, x21, [x23, #-0x18]
1000534a4:     	stur	x24, [x23, #-0x8]
1000534a8:     	b	0x1000534c4 <_aura_direct_exit_call+0x388>
1000534ac:     	tst	x20, #0x7fffffffffffffff
1000534b0:     	b.eq	0x1000534c4 <_aura_direct_exit_call+0x388>
1000534b4:     	mov	x0, x21
1000534b8:     	mov	x1, x20
1000534bc:     	mov	w2, #0x1                ; =1
1000534c0:     	bl	0x100056c88 <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
1000534c4:     	ldr	x8, [x19]
1000534c8:     	add	x8, x8, #0x1
1000534cc:     	str	x8, [x19]
1000534d0:     	ldp	x29, x30, [sp, #0x130]
1000534d4:     	ldp	x20, x19, [sp, #0x120]
1000534d8:     	ldp	x22, x21, [sp, #0x110]
1000534dc:     	ldp	x24, x23, [sp, #0x100]
1000534e0:     	ldp	x26, x25, [sp, #0xf0]
1000534e4:     	ldp	x28, x27, [sp, #0xe0]
1000534e8:     	add	sp, sp, #0x140
1000534ec:     	ret
1000534f0:     	ldr	x0, [x23, #0x38]
1000534f4:     	mov	w2, #0x1                ; =1
1000534f8:     	bl	0x100056c88 <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
1000534fc:     	cbnz	x26, 0x10005346c <_aura_direct_exit_call+0x330>
100053500:     	b	0x10005347c <_aura_direct_exit_call+0x340>
100053504:     	stp	x8, x20, [sp]
100053508:     	stp	x9, xzr, [sp, #0x10]
10005350c:     	str	x11, [sp, #0x20]
100053510:     	b	0x100053234 <_aura_direct_exit_call+0xf8>
100053514:     	adrp	x0, 0x100150000 <_writev+0x100150000>
100053518:     	add	x0, x0, #0x8c0
10005351c:     	bl	0x10011fb8c <__RNvNtCsl8K0bEFm1U0_4core4cell22panic_already_borrowed>
100053520:     	ldr	x8, [x19]
100053524:     	add	x8, x8, #0x1
100053528:     	str	x8, [x19]
10005352c:     	bl	0x10011ffd4 <_writev+0x10011ffd4>
