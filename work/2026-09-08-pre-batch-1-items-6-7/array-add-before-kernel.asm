000000010000e58c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E>:
10000e58c:     	sub	sp, sp, #0x170
10000e590:     	stp	d9, d8, [sp, #0x100]
10000e594:     	stp	x28, x27, [sp, #0x110]
10000e598:     	stp	x26, x25, [sp, #0x120]
10000e59c:     	stp	x24, x23, [sp, #0x130]
10000e5a0:     	stp	x22, x21, [sp, #0x140]
10000e5a4:     	stp	x20, x19, [sp, #0x150]
10000e5a8:     	stp	x29, x30, [sp, #0x160]
10000e5ac:     	add	x29, sp, #0x160
10000e5b0:     	mov	x22, x5
10000e5b4:     	mov	x23, x4
10000e5b8:     	mov	x20, x3
10000e5bc:     	mov	x24, x2
10000e5c0:     	mov	x21, x1
10000e5c4:     	mov	x19, x0
10000e5c8:     	cbz	x6, 0x10000e5f4 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x68>
10000e5cc:     	adrp	x1, 0x10012e000 <_aura_data_146+0x16a4>
10000e5d0:     	add	x1, x1, #0xf2c
10000e5d4:     	adrp	x2, 0x10012f000 <_aura_data_146+0x26a4>
10000e5d8:     	add	x2, x2, #0x406
10000e5dc:     	add	x0, sp, #0x40
10000e5e0:     	mov	w3, #0x43               ; =67
10000e5e4:     	bl	0x10002e2cc <__ZN13aura_compiler4diag10Diagnostic5coded17h0c8f829f44e62944E>
10000e5e8:     	ldr	x8, [sp, #0x40]
10000e5ec:     	cmp	x8, #0x2
10000e5f0:     	b.ne	0x10000e7a0 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x214>
10000e5f4:     	cbnz	x24, 0x10000e81c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x290>
10000e5f8:     	mov	w8, #0x8                ; =8
10000e5fc:     	stp	x24, x8, [sp, #0x48]
10000e600:     	str	xzr, [sp, #0x58]
10000e604:     	ldr	x8, [sp, #0x58]
10000e608:     	ldur	q0, [sp, #0x48]
10000e60c:     	str	q0, [sp]
10000e610:     	str	x8, [sp, #0x10]
10000e614:     	cmp	x23, x24
10000e618:     	csel	x24, x23, x24, lo
10000e61c:     	cbz	x24, 0x10000e784 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x1f8>
10000e620:     	cmp	x22, #0x3
10000e624:     	b.ne	0x10000e664 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0xd8>
10000e628:     	mov	x25, #0x0               ; =0
10000e62c:     	adrp	x28, 0x100037000 <__ZN3std6thread5local17LocalKey$LT$T$GT$8try_with17h1bfa53ceb25350b8E+0x120>
10000e630:     	add	x28, x28, #0xa1c
10000e634:     	add	x26, sp, #0x98
10000e638:     	adrp	x22, 0x100141000 <_writev+0x100141000>
10000e63c:     	add	x22, x22, #0xd3
10000e640:     	adrp	x23, 0x10012f000 <_aura_data_146+0x26a4>
10000e644:     	add	x23, x23, #0x28b
10000e648:     	b	0x10000e6c0 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x134>
10000e64c:     	ldr	x8, [sp, #0x8]
10000e650:     	str	d8, [x8, x23, lsl #3]
10000e654:     	add	x8, x23, #0x1
10000e658:     	str	x8, [sp, #0x10]
10000e65c:     	subs	x24, x24, #0x1
10000e660:     	b.eq	0x10000e784 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x1f8>
10000e664:     	ldr	d0, [x21], #0x8
10000e668:     	ldr	d1, [x20], #0x8
10000e66c:     	fadd	d2, d0, d1
10000e670:     	cmp	x22, #0x1
10000e674:     	fsub	d3, d0, d1
10000e678:     	fmul	d0, d0, d1
10000e67c:     	fcsel	d0, d3, d0, eq
10000e680:     	cmp	x22, #0x0
10000e684:     	fcsel	d8, d2, d0, eq
10000e688:     	ldr	x23, [sp, #0x10]
10000e68c:     	ldr	x8, [sp]
10000e690:     	cmp	x23, x8
10000e694:     	b.ne	0x10000e64c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0xc0>
10000e698:     	mov	x0, sp
10000e69c:     	bl	0x1001253c8 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hf9c8210e3d80a166E>
10000e6a0:     	b	0x10000e64c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0xc0>
10000e6a4:     	add	x25, x25, #0x1
10000e6a8:     	ldr	x8, [sp, #0x8]
10000e6ac:     	str	d8, [x8, x27, lsl #3]
10000e6b0:     	add	x8, x27, #0x1
10000e6b4:     	str	x8, [sp, #0x10]
10000e6b8:     	cmp	x24, x25
10000e6bc:     	b.eq	0x10000e784 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x1f8>
10000e6c0:     	ldr	d0, [x20, x25, lsl #3]
10000e6c4:     	fcmp	d0, #0.0
10000e6c8:     	b.ne	0x10000e754 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x1c8>
10000e6cc:     	adrp	x8, 0x10012f000 <_aura_data_146+0x26a4>
10000e6d0:     	add	x8, x8, #0x452
10000e6d4:     	stp	x25, x8, [sp, #0x98]
10000e6d8:     	mov	w8, #0x12               ; =18
10000e6dc:     	str	x8, [sp, #0xa8]
10000e6e0:     	adrp	x8, 0x100130000 <_aura_data_146+0x36a4>
10000e6e4:     	add	x9, x8, #0x9a8
10000e6e8:     	mov	w8, #0x8                ; =8
10000e6ec:     	stp	x9, x8, [sp, #0x20]
10000e6f0:     	add	x8, sp, #0x20
10000e6f4:     	stp	x8, x28, [x29, #-0x98]
10000e6f8:     	add	x8, sp, #0xa0
10000e6fc:     	stp	x8, x28, [x29, #-0x88]
10000e700:     	stur	x26, [x29, #-0x78]
10000e704:     	adrp	x8, 0x100122000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
10000e708:     	add	x8, x8, #0xc8c
10000e70c:     	stur	x8, [x29, #-0x70]
10000e710:     	add	x8, sp, #0xb0
10000e714:     	sub	x1, x29, #0x98
10000e718:     	mov	x0, x22
10000e71c:     	bl	0x10011927c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
10000e720:     	add	x0, sp, #0x40
10000e724:     	add	x2, sp, #0xb0
10000e728:     	mov	x1, x23
10000e72c:     	bl	0x10002e3a0 <__ZN13aura_compiler4diag10Diagnostic5coded17hc0c6d1b6375dce8eE>
10000e730:     	ldr	x9, [sp, #0x40]
10000e734:     	ldr	d8, [sp, #0x48]
10000e738:     	ldr	x8, [sp]
10000e73c:     	cmp	x9, #0x2
10000e740:     	b.ne	0x10000e7e4 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x258>
10000e744:     	ldr	x27, [sp, #0x10]
10000e748:     	cmp	x27, x8
10000e74c:     	b.ne	0x10000e6a4 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x118>
10000e750:     	b	0x10000e778 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x1ec>
10000e754:     	ldr	d1, [x21, x25, lsl #3]
10000e758:     	fdiv	d8, d1, d0
10000e75c:     	str	d8, [sp, #0x48]
10000e760:     	mov	w8, #0x2                ; =2
10000e764:     	str	x8, [sp, #0x40]
10000e768:     	ldr	x8, [sp]
10000e76c:     	ldr	x27, [sp, #0x10]
10000e770:     	cmp	x27, x8
10000e774:     	b.ne	0x10000e6a4 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x118>
10000e778:     	mov	x0, sp
10000e77c:     	bl	0x1001253c8 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hf9c8210e3d80a166E>
10000e780:     	b	0x10000e6a4 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x118>
10000e784:     	ldr	q0, [sp]
10000e788:     	stur	q0, [x19, #0x8]
10000e78c:     	ldr	x8, [sp, #0x10]
10000e790:     	str	x8, [x19, #0x18]
10000e794:     	mov	w8, #0x2                ; =2
10000e798:     	str	x8, [x19]
10000e79c:     	b	0x10000e7c0 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x234>
10000e7a0:     	ldp	q0, q1, [sp, #0x60]
10000e7a4:     	stp	q0, q1, [x19, #0x20]
10000e7a8:     	ldr	q0, [sp, #0x80]
10000e7ac:     	str	q0, [x19, #0x40]
10000e7b0:     	ldr	x8, [sp, #0x90]
10000e7b4:     	str	x8, [x19, #0x50]
10000e7b8:     	ldp	q0, q1, [sp, #0x40]
10000e7bc:     	stp	q0, q1, [x19]
10000e7c0:     	ldp	x29, x30, [sp, #0x160]
10000e7c4:     	ldp	x20, x19, [sp, #0x150]
10000e7c8:     	ldp	x22, x21, [sp, #0x140]
10000e7cc:     	ldp	x24, x23, [sp, #0x130]
10000e7d0:     	ldp	x26, x25, [sp, #0x120]
10000e7d4:     	ldp	x28, x27, [sp, #0x110]
10000e7d8:     	ldp	d9, d8, [sp, #0x100]
10000e7dc:     	add	sp, sp, #0x170
10000e7e0:     	ret
10000e7e4:     	ldp	q0, q1, [sp, #0x70]
10000e7e8:     	stp	q0, q1, [x19, #0x30]
10000e7ec:     	ldr	x10, [sp, #0x90]
10000e7f0:     	str	x10, [x19, #0x50]
10000e7f4:     	ldp	q1, q0, [sp, #0x50]
10000e7f8:     	stp	q1, q0, [x19, #0x10]
10000e7fc:     	str	x9, [x19]
10000e800:     	str	d8, [x19, #0x8]
10000e804:     	cbz	x8, 0x10000e7c0 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x234>
10000e808:     	ldr	x0, [sp, #0x8]
10000e80c:     	lsl	x1, x8, #3
10000e810:     	mov	w2, #0x8                ; =8
10000e814:     	bl	0x1000620b0 <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10000e818:     	b	0x10000e7c0 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x234>
10000e81c:     	sub	x0, x29, #0x98
10000e820:     	mov	x1, #0x0                ; =0
10000e824:     	mov	w2, #0x8                ; =8
10000e828:     	mov	x3, x24
10000e82c:     	mov	w4, #0x8                ; =8
10000e830:     	mov	w5, #0x8                ; =8
10000e834:     	bl	0x100125708 <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$11finish_grow17h8de8c598a5dc063cE>
10000e838:     	ldur	w8, [x29, #-0x98]
10000e83c:     	tbz	w8, #0x0, 0x10000e8e0 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x354>
10000e840:     	adrp	x8, 0x10012f000 <_aura_data_146+0x26a4>
10000e844:     	add	x8, x8, #0x201
10000e848:     	mov	w9, #0x17               ; =23
10000e84c:     	stp	x8, x9, [sp, #0xa0]
10000e850:     	str	x24, [sp, #0x98]
10000e854:     	add	x8, sp, #0xa0
10000e858:     	adrp	x9, 0x100037000 <__ZN3std6thread5local17LocalKey$LT$T$GT$8try_with17h1bfa53ceb25350b8E+0x120>
10000e85c:     	add	x9, x9, #0xa1c
10000e860:     	stp	x8, x9, [x29, #-0x98]
10000e864:     	add	x8, sp, #0x98
10000e868:     	adrp	x9, 0x100122000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
10000e86c:     	add	x9, x9, #0xc8c
10000e870:     	stp	x8, x9, [x29, #-0x88]
10000e874:     	adrp	x0, 0x100141000 <_writev+0x100141000>
10000e878:     	add	x0, x0, #0x9f
10000e87c:     	add	x8, sp, #0xb0
10000e880:     	sub	x1, x29, #0x98
10000e884:     	bl	0x10011927c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
10000e888:     	adrp	x1, 0x10012e000 <_aura_data_146+0x16a4>
10000e88c:     	add	x1, x1, #0xf53
10000e890:     	add	x0, sp, #0x40
10000e894:     	add	x2, sp, #0xb0
10000e898:     	bl	0x10002e3a0 <__ZN13aura_compiler4diag10Diagnostic5coded17hc0c6d1b6375dce8eE>
10000e89c:     	ldr	x8, [sp, #0x40]
10000e8a0:     	cmp	x8, #0x2
10000e8a4:     	b.eq	0x10000e604 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x78>
10000e8a8:     	ldur	q0, [sp, #0x48]
10000e8ac:     	str	q0, [sp, #0x20]
10000e8b0:     	ldr	x9, [sp, #0x58]
10000e8b4:     	str	x9, [sp, #0x30]
10000e8b8:     	ldp	q1, q2, [sp, #0x60]
10000e8bc:     	stp	q1, q2, [x19, #0x20]
10000e8c0:     	ldr	q1, [sp, #0x80]
10000e8c4:     	str	q1, [x19, #0x40]
10000e8c8:     	ldr	x10, [sp, #0x90]
10000e8cc:     	str	x10, [x19, #0x50]
10000e8d0:     	stur	q0, [x19, #0x8]
10000e8d4:     	str	x9, [x19, #0x18]
10000e8d8:     	str	x8, [x19]
10000e8dc:     	b	0x10000e7c0 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x234>
10000e8e0:     	ldur	x8, [x29, #-0x90]
10000e8e4:     	b	0x10000e5fc <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x70>
10000e8e8:     	b	0x10000e8ec <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x360>
10000e8ec:     	mov	x19, x0
10000e8f0:     	ldr	x8, [sp]
10000e8f4:     	cbz	x8, 0x10000e908 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x37c>
10000e8f8:     	ldr	x0, [sp, #0x8]
10000e8fc:     	lsl	x1, x8, #3
10000e900:     	mov	w2, #0x8                ; =8
10000e904:     	bl	0x1000620b0 <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10000e908:     	mov	x0, x19
10000e90c:     	bl	0x10012c1c0 <_writev+0x10012c1c0>
