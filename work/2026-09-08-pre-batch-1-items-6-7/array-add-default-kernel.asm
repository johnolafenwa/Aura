0000000100018294 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE>:
100018294:     	sub	sp, sp, #0x170
100018298:     	stp	d9, d8, [sp, #0x100]
10001829c:     	stp	x28, x27, [sp, #0x110]
1000182a0:     	stp	x26, x25, [sp, #0x120]
1000182a4:     	stp	x24, x23, [sp, #0x130]
1000182a8:     	stp	x22, x21, [sp, #0x140]
1000182ac:     	stp	x20, x19, [sp, #0x150]
1000182b0:     	stp	x29, x30, [sp, #0x160]
1000182b4:     	add	x29, sp, #0x160
1000182b8:     	mov	x22, x5
1000182bc:     	mov	x23, x4
1000182c0:     	mov	x20, x3
1000182c4:     	mov	x24, x2
1000182c8:     	mov	x21, x1
1000182cc:     	mov	x19, x0
1000182d0:     	cbz	x6, 0x100018300 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x6c>
1000182d4:     	adrp	x0, 0x100146000 <_aura_data_146+0xa4>
1000182d8:     	add	x0, x0, #0xe6b
1000182dc:     	adrp	x2, 0x100147000 <_anon.ddfd7358f93c9a77e2ee434241d772e6.264+0xd8>
1000182e0:     	add	x2, x2, #0x104
1000182e4:     	add	x8, sp, #0x40
1000182e8:     	mov	w1, #0x6                ; =6
1000182ec:     	mov	w3, #0x43               ; =67
1000182f0:     	bl	0x100069d4c <__ZN13aura_compiler4diag10Diagnostic5coded17h95f54800888bbdc8E>
1000182f4:     	ldr	x8, [sp, #0x40]
1000182f8:     	cmp	x8, #0x2
1000182fc:     	b.ne	0x100018384 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0xf0>
100018300:     	cbnz	x24, 0x1000185a8 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x314>
100018304:     	mov	w8, #0x8                ; =8
100018308:     	add	x9, sp, #0x40
10001830c:     	add	x9, x9, #0x8
100018310:     	stp	x24, x8, [sp, #0x48]
100018314:     	str	xzr, [sp, #0x58]
100018318:     	ldr	x8, [x9, #0x10]
10001831c:     	ldr	q0, [x9]
100018320:     	str	q0, [sp]
100018324:     	str	x8, [sp, #0x10]
100018328:     	cmp	x23, x24
10001832c:     	csel	x24, x23, x24, lo
100018330:     	cbz	x24, 0x100018530 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x29c>
100018334:     	cmp	x22, #0x1
100018338:     	b.gt	0x1000183a8 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x114>
10001833c:     	cbnz	x22, 0x1000183f4 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x160>
100018340:     	ldr	x22, [sp, #0x10]
100018344:     	b	0x100018364 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0xd0>
100018348:     	fadd	d0, d8, d9
10001834c:     	ldr	x8, [sp, #0x8]
100018350:     	str	d0, [x8, x22, lsl #3]
100018354:     	add	x22, x22, #0x1
100018358:     	str	x22, [sp, #0x10]
10001835c:     	subs	x24, x24, #0x1
100018360:     	b.eq	0x100018530 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x29c>
100018364:     	ldr	d8, [x21], #0x8
100018368:     	ldr	d9, [x20], #0x8
10001836c:     	ldr	x8, [sp]
100018370:     	cmp	x22, x8
100018374:     	b.ne	0x100018348 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0xb4>
100018378:     	mov	x0, sp
10001837c:     	bl	0x10013cdb0 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hed6adb543f29affbE>
100018380:     	b	0x100018348 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0xb4>
100018384:     	ldp	q0, q1, [sp, #0x60]
100018388:     	stp	q0, q1, [x19, #0x20]
10001838c:     	ldr	q0, [sp, #0x80]
100018390:     	str	q0, [x19, #0x40]
100018394:     	ldr	x8, [sp, #0x90]
100018398:     	str	x8, [x19, #0x50]
10001839c:     	ldp	q0, q1, [sp, #0x40]
1000183a0:     	stp	q0, q1, [x19]
1000183a4:     	b	0x100018548 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x2b4>
1000183a8:     	cmp	x22, #0x2
1000183ac:     	b.ne	0x100018438 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x1a4>
1000183b0:     	ldr	x22, [sp, #0x10]
1000183b4:     	b	0x1000183d4 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x140>
1000183b8:     	fmul	d0, d8, d9
1000183bc:     	ldr	x8, [sp, #0x8]
1000183c0:     	str	d0, [x8, x22, lsl #3]
1000183c4:     	add	x22, x22, #0x1
1000183c8:     	str	x22, [sp, #0x10]
1000183cc:     	subs	x24, x24, #0x1
1000183d0:     	b.eq	0x100018530 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x29c>
1000183d4:     	ldr	d8, [x21], #0x8
1000183d8:     	ldr	d9, [x20], #0x8
1000183dc:     	ldr	x8, [sp]
1000183e0:     	cmp	x22, x8
1000183e4:     	b.ne	0x1000183b8 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x124>
1000183e8:     	mov	x0, sp
1000183ec:     	bl	0x10013cdb0 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hed6adb543f29affbE>
1000183f0:     	b	0x1000183b8 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x124>
1000183f4:     	ldr	x22, [sp, #0x10]
1000183f8:     	b	0x100018418 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x184>
1000183fc:     	fsub	d0, d8, d9
100018400:     	ldr	x8, [sp, #0x8]
100018404:     	str	d0, [x8, x22, lsl #3]
100018408:     	add	x22, x22, #0x1
10001840c:     	str	x22, [sp, #0x10]
100018410:     	subs	x24, x24, #0x1
100018414:     	b.eq	0x100018530 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x29c>
100018418:     	ldr	d8, [x21], #0x8
10001841c:     	ldr	d9, [x20], #0x8
100018420:     	ldr	x8, [sp]
100018424:     	cmp	x22, x8
100018428:     	b.ne	0x1000183fc <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x168>
10001842c:     	mov	x0, sp
100018430:     	bl	0x10013cdb0 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hed6adb543f29affbE>
100018434:     	b	0x1000183fc <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x168>
100018438:     	mov	x25, #0x0               ; =0
10001843c:     	adrp	x28, 0x100063000 <__ZN10serde_core3ser12SerializeMap15serialize_entry17hf43616cc5c13af4dE+0x38>
100018440:     	add	x28, x28, #0x834
100018444:     	adrp	x26, 0x10013a000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
100018448:     	add	x26, x26, #0xc8c
10001844c:     	adrp	x22, 0x10015b000 <GCC_except_table671+0x40>
100018450:     	add	x22, x22, #0x974
100018454:     	adrp	x23, 0x100146000 <_aura_data_146+0xa4>
100018458:     	add	x23, x23, #0xf82
10001845c:     	b	0x10001847c <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x1e8>
100018460:     	add	x25, x25, #0x1
100018464:     	ldr	x8, [sp, #0x8]
100018468:     	str	d8, [x8, x27, lsl #3]
10001846c:     	add	x8, x27, #0x1
100018470:     	str	x8, [sp, #0x10]
100018474:     	cmp	x24, x25
100018478:     	b.eq	0x100018530 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x29c>
10001847c:     	ldr	d0, [x20, x25, lsl #3]
100018480:     	fcmp	d0, #0.0
100018484:     	b.ne	0x10001850c <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x278>
100018488:     	adrp	x8, 0x100147000 <_anon.ddfd7358f93c9a77e2ee434241d772e6.264+0xd8>
10001848c:     	add	x8, x8, #0x14e
100018490:     	stur	x8, [x29, #-0x98]
100018494:     	mov	w8, #0x12               ; =18
100018498:     	stp	x8, x25, [x29, #-0x90]
10001849c:     	adrp	x8, 0x100147000 <_anon.ddfd7358f93c9a77e2ee434241d772e6.264+0xd8>
1000184a0:     	add	x9, x8, #0x490
1000184a4:     	mov	w8, #0x8                ; =8
1000184a8:     	stp	x9, x8, [sp, #0x20]
1000184ac:     	add	x8, sp, #0x20
1000184b0:     	stp	x8, x28, [sp, #0x98]
1000184b4:     	sub	x8, x29, #0x98
1000184b8:     	stp	x8, x28, [sp, #0xa8]
1000184bc:     	sub	x8, x29, #0x88
1000184c0:     	stp	x8, x26, [sp, #0xb8]
1000184c4:     	sub	x8, x29, #0x80
1000184c8:     	add	x1, sp, #0x98
1000184cc:     	mov	x0, x22
1000184d0:     	bl	0x10013127c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
1000184d4:     	add	x8, sp, #0x40
1000184d8:     	sub	x2, x29, #0x80
1000184dc:     	mov	x0, x23
1000184e0:     	mov	w1, #0x6                ; =6
1000184e4:     	bl	0x100069c54 <__ZN13aura_compiler4diag10Diagnostic5coded17h91c752b3468779edE>
1000184e8:     	ldr	x8, [sp, #0x40]
1000184ec:     	ldr	d8, [sp, #0x48]
1000184f0:     	cmp	x8, #0x2
1000184f4:     	b.ne	0x10001856c <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x2d8>
1000184f8:     	ldr	x27, [sp, #0x10]
1000184fc:     	ldr	x8, [sp]
100018500:     	cmp	x27, x8
100018504:     	b.ne	0x100018460 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x1cc>
100018508:     	b	0x100018524 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x290>
10001850c:     	ldr	d1, [x21, x25, lsl #3]
100018510:     	fdiv	d8, d1, d0
100018514:     	ldr	x27, [sp, #0x10]
100018518:     	ldr	x8, [sp]
10001851c:     	cmp	x27, x8
100018520:     	b.ne	0x100018460 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x1cc>
100018524:     	mov	x0, sp
100018528:     	bl	0x10013cdb0 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hed6adb543f29affbE>
10001852c:     	b	0x100018460 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x1cc>
100018530:     	ldr	q0, [sp]
100018534:     	stur	q0, [x19, #0x8]
100018538:     	ldr	x8, [sp, #0x10]
10001853c:     	str	x8, [x19, #0x18]
100018540:     	mov	w8, #0x2                ; =2
100018544:     	str	x8, [x19]
100018548:     	ldp	x29, x30, [sp, #0x160]
10001854c:     	ldp	x20, x19, [sp, #0x150]
100018550:     	ldp	x22, x21, [sp, #0x140]
100018554:     	ldp	x24, x23, [sp, #0x130]
100018558:     	ldp	x26, x25, [sp, #0x120]
10001855c:     	ldp	x28, x27, [sp, #0x110]
100018560:     	ldp	d9, d8, [sp, #0x100]
100018564:     	add	sp, sp, #0x170
100018568:     	ret
10001856c:     	ldp	q0, q1, [sp, #0x70]
100018570:     	stp	q0, q1, [x19, #0x30]
100018574:     	ldr	x9, [sp, #0x90]
100018578:     	str	x9, [x19, #0x50]
10001857c:     	ldp	q1, q0, [sp, #0x50]
100018580:     	stp	q1, q0, [x19, #0x10]
100018584:     	str	x8, [x19]
100018588:     	str	d8, [x19, #0x8]
10001858c:     	ldr	x8, [sp]
100018590:     	cbz	x8, 0x100018548 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x2b4>
100018594:     	ldr	x0, [sp, #0x8]
100018598:     	lsl	x1, x8, #3
10001859c:     	mov	w2, #0x8                ; =8
1000185a0:     	bl	0x10007906c <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
1000185a4:     	b	0x100018548 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x2b4>
1000185a8:     	add	x0, sp, #0x98
1000185ac:     	mov	x1, #0x0                ; =0
1000185b0:     	mov	w2, #0x8                ; =8
1000185b4:     	mov	x3, x24
1000185b8:     	mov	w4, #0x8                ; =8
1000185bc:     	mov	w5, #0x8                ; =8
1000185c0:     	bl	0x10013d1c0 <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$11finish_grow17hbe7f5886cc698ff0E>
1000185c4:     	ldr	w8, [sp, #0x98]
1000185c8:     	tbz	w8, #0x0, 0x100018674 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x3e0>
1000185cc:     	adrp	x8, 0x100146000 <_aura_data_146+0xa4>
1000185d0:     	add	x8, x8, #0xf39
1000185d4:     	mov	w9, #0x17               ; =23
1000185d8:     	stp	x8, x9, [x29, #-0x98]
1000185dc:     	stur	x24, [x29, #-0x88]
1000185e0:     	sub	x8, x29, #0x98
1000185e4:     	adrp	x9, 0x100063000 <__ZN10serde_core3ser12SerializeMap15serialize_entry17hf43616cc5c13af4dE+0x38>
1000185e8:     	add	x9, x9, #0x834
1000185ec:     	stp	x8, x9, [sp, #0x98]
1000185f0:     	sub	x8, x29, #0x88
1000185f4:     	adrp	x9, 0x10013a000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
1000185f8:     	add	x9, x9, #0xc8c
1000185fc:     	stp	x8, x9, [sp, #0xa8]
100018600:     	adrp	x0, 0x10015b000 <GCC_except_table671+0x40>
100018604:     	add	x0, x0, #0x940
100018608:     	sub	x8, x29, #0x80
10001860c:     	add	x1, sp, #0x98
100018610:     	bl	0x10013127c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
100018614:     	adrp	x0, 0x100146000 <_aura_data_146+0xa4>
100018618:     	add	x0, x0, #0xeb6
10001861c:     	add	x25, sp, #0x40
100018620:     	add	x8, sp, #0x40
100018624:     	sub	x2, x29, #0x80
100018628:     	mov	w1, #0x6                ; =6
10001862c:     	bl	0x100069c54 <__ZN13aura_compiler4diag10Diagnostic5coded17h91c752b3468779edE>
100018630:     	ldr	x8, [sp, #0x40]
100018634:     	add	x9, x25, #0x8
100018638:     	cmp	x8, #0x2
10001863c:     	b.eq	0x100018318 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x84>
100018640:     	ldr	q0, [x9]
100018644:     	str	q0, [sp, #0x20]
100018648:     	ldr	x9, [x9, #0x10]
10001864c:     	str	x9, [sp, #0x30]
100018650:     	ldp	q1, q2, [sp, #0x60]
100018654:     	stp	q1, q2, [x19, #0x20]
100018658:     	ldr	q1, [sp, #0x80]
10001865c:     	str	q1, [x19, #0x40]
100018660:     	ldr	x10, [sp, #0x90]
100018664:     	str	x10, [x19, #0x50]
100018668:     	stur	q0, [x19, #0x8]
10001866c:     	str	x9, [x19, #0x18]
100018670:     	b	0x100018544 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x2b0>
100018674:     	ldr	x8, [sp, #0xa0]
100018678:     	b	0x100018308 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x74>
10001867c:     	b	0x100018688 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x3f4>
100018680:     	b	0x100018688 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x3f4>
100018684:     	b	0x100018688 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x3f4>
100018688:     	mov	x19, x0
10001868c:     	ldr	x8, [sp]
100018690:     	cbz	x8, 0x1000186a4 <__ZN13aura_compiler13runtime_value20array_float64_binary17hba8582997b887febE+0x410>
100018694:     	ldr	x0, [sp, #0x8]
100018698:     	lsl	x1, x8, #3
10001869c:     	mov	w2, #0x8                ; =8
1000186a0:     	bl	0x10007906c <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
1000186a4:     	mov	x0, x19
1000186a8:     	bl	0x1001457d0 <_writev+0x1001457d0>
