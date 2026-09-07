0000000100009ee0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE>:
100009ee0:     	stp	x28, x27, [sp, #-0x60]!
100009ee4:     	stp	x26, x25, [sp, #0x10]
100009ee8:     	stp	x24, x23, [sp, #0x20]
100009eec:     	stp	x22, x21, [sp, #0x30]
100009ef0:     	stp	x20, x19, [sp, #0x40]
100009ef4:     	stp	x29, x30, [sp, #0x50]
100009ef8:     	add	x29, sp, #0x50
100009efc:     	sub	sp, sp, #0x1e0
100009f00:     	mov	x24, x4
100009f04:     	mov	x23, x3
100009f08:     	mov	x26, x2
100009f0c:     	mov	x25, x1
100009f10:     	mov	x22, x0
100009f14:     	mov	x21, x8
100009f18:     	add	x28, sp, #0xf0
100009f1c:     	ldp	x1, x2, [x0, #0x18]
100009f20:     	adrp	x3, 0x10012f000 <_aura_data_146+0x26a4>
100009f24:     	add	x3, x3, #0x146
100009f28:     	add	x0, sp, #0xf0
100009f2c:     	mov	w4, #0xb                ; =11
100009f30:     	bl	0x10000ff54 <__ZN13aura_compiler13runtime_value22try_copy_array_storage17h09d2ee219d83f501E>
100009f34:     	ldp	x8, x19, [sp, #0xf0]
100009f38:     	ldr	x20, [sp, #0x100]
100009f3c:     	cmp	x8, #0x2
100009f40:     	b.ne	0x100009fbc <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xdc>
100009f44:     	stp	x20, x21, [sp, #0x28]
100009f48:     	ldr	x8, [x22]
100009f4c:     	cmp	x8, #0x1
100009f50:     	str	x19, [sp, #0x20]
100009f54:     	b.gt	0x100009fe8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x108>
100009f58:     	str	x8, [sp, #0x8]
100009f5c:     	cbnz	x8, 0x10000a01c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x13c>
100009f60:     	add	x0, sp, #0xf0
100009f64:     	mov	x1, x25
100009f68:     	mov	x2, #0x0                ; =0
100009f6c:     	bl	0x10000de10 <__ZN13aura_compiler13runtime_value18array_int32_scalar17h87f81693e2079d66E>
100009f70:     	ldr	x8, [sp, #0xf0]
100009f74:     	ldr	w21, [sp, #0xf8]
100009f78:     	cmp	x8, #0x2
100009f7c:     	b.ne	0x10000a380 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x4a0>
100009f80:     	ldp	x8, x25, [x22, #0x8]
100009f84:     	str	x8, [sp, #0x18]
100009f88:     	cbnz	x25, 0x10000aa1c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xb3c>
100009f8c:     	mov	w20, #0x4               ; =4
100009f90:     	stp	x25, x20, [x29, #-0xb0]
100009f94:     	stur	xzr, [x29, #-0xa0]
100009f98:     	cbz	x25, 0x10000af98 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x10b8>
100009f9c:     	lsl	x19, x25, #2
100009fa0:     	str	x21, [sp, #0x10]
100009fa4:     	mov	x25, #0x0               ; =0
100009fa8:     	tbz	w26, #0x0, 0x10000aac4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xbe4>
100009fac:     	sxtw	x26, w21
100009fb0:     	asr	x28, x26, #63
100009fb4:     	mov	x21, #-0x1              ; =-1
100009fb8:     	b	0x10000a0b0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1d0>
100009fbc:     	ldur	q0, [x28, #0x18]
100009fc0:     	stur	q0, [x21, #0x18]
100009fc4:     	ldur	q0, [x28, #0x28]
100009fc8:     	stur	q0, [x21, #0x28]
100009fcc:     	ldur	q0, [x28, #0x38]
100009fd0:     	stur	q0, [x21, #0x38]
100009fd4:     	ldur	q0, [x28, #0x48]
100009fd8:     	stur	q0, [x21, #0x48]
100009fdc:     	stp	x8, x19, [x21]
100009fe0:     	str	x20, [x21, #0x10]
100009fe4:     	b	0x10000b960 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a80>
100009fe8:     	mov	x21, x8
100009fec:     	cmp	x8, #0x2
100009ff0:     	b.ne	0x10000a070 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x190>
100009ff4:     	ldr	x8, [x25]
100009ff8:     	mov	x9, #-0x7fffffffffffffff ; =-9223372036854775807
100009ffc:     	cmp	x8, x9
10000a000:     	b.ne	0x10000a208 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x328>
10000a004:     	ldr	d0, [x25, #0x8]
10000a008:     	fcvt	s0, d0
10000a00c:     	ldp	x27, x25, [x22, #0x8]
10000a010:     	stur	s0, [x29, #-0xb8]
10000a014:     	cbnz	x24, 0x10000a23c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x35c>
10000a018:     	b	0x10000a26c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x38c>
10000a01c:     	add	x0, sp, #0xf0
10000a020:     	mov	x1, x25
10000a024:     	mov	x2, #0x0                ; =0
10000a028:     	bl	0x10000dfa0 <__ZN13aura_compiler13runtime_value18array_int64_scalar17h2fef89b47337f3c5E>
10000a02c:     	ldp	x8, x28, [sp, #0xf0]
10000a030:     	cmp	x8, #0x2
10000a034:     	b.ne	0x10000a3bc <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x4dc>
10000a038:     	ldp	x8, x27, [x22, #0x8]
10000a03c:     	str	x8, [sp, #0x18]
10000a040:     	cbnz	x27, 0x10000acd4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xdf4>
10000a044:     	mov	w25, #0x8               ; =8
10000a048:     	stp	x27, x25, [x29, #-0xb0]
10000a04c:     	stur	xzr, [x29, #-0xa0]
10000a050:     	cbz	x27, 0x10000b014 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1134>
10000a054:     	lsl	x19, x27, #3
10000a058:     	tbz	w26, #0x0, 0x10000ad74 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xe94>
10000a05c:     	mov	x26, #0x0               ; =0
10000a060:     	lsr	x8, x28, #63
10000a064:     	eor	w20, w8, #0x1
10000a068:     	asr	x21, x28, #63
10000a06c:     	b	0x10000a170 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x290>
10000a070:     	ldr	x8, [x25]
10000a074:     	mov	x9, #-0x7fffffffffffffff ; =-9223372036854775807
10000a078:     	cmp	x8, x9
10000a07c:     	b.ne	0x10000a2c4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x3e4>
10000a080:     	ldr	d0, [x25, #0x8]
10000a084:     	ldp	x27, x25, [x22, #0x8]
10000a088:     	stur	d0, [x29, #-0xb8]
10000a08c:     	cbnz	x24, 0x10000a2f8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x418>
10000a090:     	b	0x10000a328 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x448>
10000a094:     	ldur	x20, [x29, #-0xa8]
10000a098:     	str	w22, [x20, x27, lsl #2]
10000a09c:     	add	x22, x27, #0x1
10000a0a0:     	stur	x22, [x29, #-0xa0]
10000a0a4:     	add	x25, x25, #0x1
10000a0a8:     	subs	x19, x19, #0x4
10000a0ac:     	b.eq	0x10000af9c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x10bc>
10000a0b0:     	ldp	x9, x8, [sp, #0x10]
10000a0b4:     	ldrsw	x8, [x8, x25, lsl #2]
10000a0b8:     	mvn	w9, w9
10000a0bc:     	lsr	w9, w9, #31
10000a0c0:     	stp	x9, xzr, [sp, #0xc0]
10000a0c4:     	asr	x9, x8, #63
10000a0c8:     	stp	x26, x28, [sp, #0xd0]
10000a0cc:     	mov	w11, #0x2               ; =2
10000a0d0:     	strb	w11, [sp, #0xe0]
10000a0d4:     	mvn	w10, w8
10000a0d8:     	lsr	w10, w10, #31
10000a0dc:     	stp	x10, xzr, [sp, #0x80]
10000a0e0:     	stp	x8, x9, [sp, #0x90]
10000a0e4:     	strb	w11, [sp, #0xa0]
10000a0e8:     	add	x0, sp, #0xf0
10000a0ec:     	add	x1, sp, #0xc0
10000a0f0:     	add	x2, sp, #0x80
10000a0f4:     	mov	x3, x23
10000a0f8:     	mov	x4, x24
10000a0fc:     	mov	x5, x25
10000a100:     	bl	0x100017ce0 <__ZN13aura_compiler13runtime_value29apply_integer_array_operation17h418583a9cdd5b1bcE>
10000a104:     	ldr	w8, [sp, #0xf0]
10000a108:     	tbnz	w8, #0x0, 0x10000ab98 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xcb8>
10000a10c:     	ldr	w9, [sp, #0x100]
10000a110:     	ldp	x22, x8, [sp, #0x110]
10000a114:     	tbz	w9, #0x0, 0x10000a11c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x23c>
10000a118:     	tbnz	x8, #0x3f, 0x10000b0bc <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x11dc>
10000a11c:     	mov	x9, #-0x80000000        ; =-2147483648
10000a120:     	adds	x9, x22, x9
10000a124:     	adc	x8, x8, x21
10000a128:     	mov	x10, #-0x100000000      ; =-4294967296
10000a12c:     	cmp	x9, x10
10000a130:     	sbcs	xzr, x8, x21
10000a134:     	b.lo	0x10000abcc <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xcec>
10000a138:     	ldur	x27, [x29, #-0xa0]
10000a13c:     	ldur	x8, [x29, #-0xb0]
10000a140:     	cmp	x27, x8
10000a144:     	b.ne	0x10000a098 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b8>
10000a148:     	sub	x0, x29, #0xb0
10000a14c:     	bl	0x100125360 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hcd4bc81e77a6b15aE>
10000a150:     	b	0x10000a094 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b4>
10000a154:     	ldur	x25, [x29, #-0xa8]
10000a158:     	str	x22, [x25, x27, lsl #3]
10000a15c:     	add	x22, x27, #0x1
10000a160:     	stur	x22, [x29, #-0xa0]
10000a164:     	add	x26, x26, #0x1
10000a168:     	subs	x19, x19, #0x8
10000a16c:     	b.eq	0x10000b018 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1138>
10000a170:     	ldr	x8, [sp, #0x18]
10000a174:     	ldr	x8, [x8, x26, lsl #3]
10000a178:     	asr	x9, x8, #63
10000a17c:     	stp	x20, xzr, [sp, #0xc0]
10000a180:     	stp	x28, x21, [sp, #0xd0]
10000a184:     	mov	w11, #0x3               ; =3
10000a188:     	strb	w11, [sp, #0xe0]
10000a18c:     	lsr	x10, x8, #63
10000a190:     	eor	w10, w10, #0x1
10000a194:     	stp	x10, xzr, [sp, #0x80]
10000a198:     	stp	x8, x9, [sp, #0x90]
10000a19c:     	strb	w11, [sp, #0xa0]
10000a1a0:     	add	x0, sp, #0xf0
10000a1a4:     	add	x1, sp, #0xc0
10000a1a8:     	add	x2, sp, #0x80
10000a1ac:     	mov	x3, x23
10000a1b0:     	mov	x4, x24
10000a1b4:     	mov	x5, x26
10000a1b8:     	bl	0x100017ce0 <__ZN13aura_compiler13runtime_value29apply_integer_array_operation17h418583a9cdd5b1bcE>
10000a1bc:     	ldr	w8, [sp, #0xf0]
10000a1c0:     	tbnz	w8, #0x0, 0x10000ae40 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xf60>
10000a1c4:     	ldr	w9, [sp, #0x100]
10000a1c8:     	ldp	x22, x8, [sp, #0x110]
10000a1cc:     	tbz	w9, #0x0, 0x10000a1d4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x2f4>
10000a1d0:     	tbnz	x8, #0x3f, 0x10000b0d8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x11f8>
10000a1d4:     	mov	x9, #-0x8000000000000000 ; =-9223372036854775808
10000a1d8:     	cmn	x22, x9
10000a1dc:     	mov	x9, #-0x1               ; =-1
10000a1e0:     	adc	x8, x8, x9
10000a1e4:     	cmn	x8, #0x1
10000a1e8:     	b.ne	0x10000ae68 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xf88>
10000a1ec:     	ldur	x27, [x29, #-0xa0]
10000a1f0:     	ldur	x8, [x29, #-0xb0]
10000a1f4:     	cmp	x27, x8
10000a1f8:     	b.ne	0x10000a158 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x278>
10000a1fc:     	sub	x0, x29, #0xb0
10000a200:     	bl	0x1001253c8 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hf9c8210e3d80a166E>
10000a204:     	b	0x10000a154 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x274>
10000a208:     	add	x0, sp, #0xf0
10000a20c:     	mov	w1, #0x2                ; =2
10000a210:     	mov	x2, x25
10000a214:     	mov	x3, #0x0                ; =0
10000a218:     	bl	0x10000db50 <__ZN13aura_compiler13runtime_value17array_dtype_error17h9a8c8440b7635001E>
10000a21c:     	ldr	x8, [sp, #0xf0]
10000a220:     	ldr	s0, [sp, #0xf8]
10000a224:     	cmp	x8, #0x2
10000a228:     	b.ne	0x10000a438 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x558>
10000a22c:     	add	x28, sp, #0xf0
10000a230:     	ldp	x27, x25, [x22, #0x8]
10000a234:     	stur	s0, [x29, #-0xb8]
10000a238:     	cbz	x24, 0x10000a26c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x38c>
10000a23c:     	adrp	x1, 0x10012e000 <_aura_data_146+0x16a4>
10000a240:     	add	x1, x1, #0xf2c
10000a244:     	adrp	x2, 0x10012f000 <_aura_data_146+0x26a4>
10000a248:     	add	x2, x2, #0x406
10000a24c:     	add	x0, sp, #0xf0
10000a250:     	mov	w3, #0x43               ; =67
10000a254:     	ldr	x20, [sp, #0x28]
10000a258:     	bl	0x10002ef38 <__ZN13aura_compiler4diag10Diagnostic5coded17h0c8f829f44e62944E>
10000a25c:     	ldr	x20, [sp, #0xf0]
10000a260:     	cmp	x20, #0x2
10000a264:     	add	x28, sp, #0xf0
10000a268:     	b.ne	0x10000a3e4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x504>
10000a26c:     	cbnz	x25, 0x10000a594 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x6b4>
10000a270:     	mov	w24, #0x4               ; =4
10000a274:     	mov	x22, #0x0               ; =0
10000a278:     	stp	x25, x24, [sp, #0xf8]
10000a27c:     	mov	x19, x25
10000a280:     	stp	x25, x24, [x29, #-0xb0]
10000a284:     	stur	x22, [x29, #-0xa0]
10000a288:     	cmp	x23, #0x1
10000a28c:     	b.gt	0x10000a63c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x75c>
10000a290:     	sub	x8, x19, x22
10000a294:     	cmp	x25, x8
10000a298:     	cbnz	x23, 0x10000a4b4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x5d4>
10000a29c:     	tbz	w26, #0x0, 0x10000a504 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x624>
10000a2a0:     	b.hi	0x10000b0f4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1214>
10000a2a4:     	mov	x26, x21
10000a2a8:     	cbz	x25, 0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000a2ac:     	ldur	s0, [x29, #-0xb8]
10000a2b0:     	cmp	x25, #0x10
10000a2b4:     	b.hs	0x10000b120 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1240>
10000a2b8:     	mov	x8, #0x0                ; =0
10000a2bc:     	mov	x9, x22
10000a2c0:     	b	0x10000b170 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1290>
10000a2c4:     	add	x0, sp, #0xf0
10000a2c8:     	mov	w1, #0x3                ; =3
10000a2cc:     	mov	x2, x25
10000a2d0:     	mov	x3, #0x0                ; =0
10000a2d4:     	bl	0x10000db50 <__ZN13aura_compiler13runtime_value17array_dtype_error17h9a8c8440b7635001E>
10000a2d8:     	ldr	x8, [sp, #0xf0]
10000a2dc:     	ldr	d0, [sp, #0xf8]
10000a2e0:     	cmp	x8, #0x2
10000a2e4:     	b.ne	0x10000a474 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x594>
10000a2e8:     	add	x28, sp, #0xf0
10000a2ec:     	ldp	x27, x25, [x22, #0x8]
10000a2f0:     	stur	d0, [x29, #-0xb8]
10000a2f4:     	cbz	x24, 0x10000a328 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x448>
10000a2f8:     	adrp	x1, 0x10012e000 <_aura_data_146+0x16a4>
10000a2fc:     	add	x1, x1, #0xf2c
10000a300:     	adrp	x2, 0x10012f000 <_aura_data_146+0x26a4>
10000a304:     	add	x2, x2, #0x406
10000a308:     	add	x0, sp, #0xf0
10000a30c:     	mov	w3, #0x43               ; =67
10000a310:     	ldr	x20, [sp, #0x28]
10000a314:     	bl	0x10002ef38 <__ZN13aura_compiler4diag10Diagnostic5coded17h0c8f829f44e62944E>
10000a318:     	ldr	x20, [sp, #0xf0]
10000a31c:     	cmp	x20, #0x2
10000a320:     	add	x28, sp, #0xf0
10000a324:     	b.ne	0x10000a3e4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x504>
10000a328:     	cbnz	x25, 0x10000a7d8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x8f8>
10000a32c:     	mov	w24, #0x8               ; =8
10000a330:     	mov	x22, #0x0               ; =0
10000a334:     	stp	x25, x24, [sp, #0xf8]
10000a338:     	mov	x19, x25
10000a33c:     	stp	x25, x24, [x29, #-0xb0]
10000a340:     	stur	x22, [x29, #-0xa0]
10000a344:     	cmp	x23, #0x1
10000a348:     	b.gt	0x10000a880 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x9a0>
10000a34c:     	sub	x8, x19, x22
10000a350:     	cmp	x25, x8
10000a354:     	cbnz	x23, 0x10000a4dc <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x5fc>
10000a358:     	tbz	w26, #0x0, 0x10000a528 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x648>
10000a35c:     	b.hi	0x10000b198 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x12b8>
10000a360:     	mov	x26, x21
10000a364:     	cbz	x25, 0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000a368:     	ldur	d0, [x29, #-0xb8]
10000a36c:     	cmp	x25, #0x8
10000a370:     	b.hs	0x10000b1c4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x12e4>
10000a374:     	mov	x8, #0x0                ; =0
10000a378:     	mov	x9, x22
10000a37c:     	b	0x10000b214 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1334>
10000a380:     	add	x10, sp, #0xf0
10000a384:     	ldur	q0, [x10, #0x1c]
10000a388:     	ldr	x9, [sp, #0x30]
10000a38c:     	stur	q0, [x9, #0x1c]
10000a390:     	ldur	q0, [x10, #0x2c]
10000a394:     	stur	q0, [x9, #0x2c]
10000a398:     	ldur	q0, [x10, #0x3c]
10000a39c:     	stur	q0, [x9, #0x3c]
10000a3a0:     	ldur	q0, [x10, #0x48]
10000a3a4:     	stur	q0, [x9, #0x48]
10000a3a8:     	ldur	q0, [x10, #0xc]
10000a3ac:     	stur	q0, [x9, #0xc]
10000a3b0:     	str	x8, [x9]
10000a3b4:     	str	w21, [x9, #0x8]
10000a3b8:     	b	0x10000a49c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x5bc>
10000a3bc:     	add	x11, sp, #0xf0
10000a3c0:     	ldp	q0, q1, [x11, #0x30]
10000a3c4:     	ldr	x10, [sp, #0x30]
10000a3c8:     	stp	q0, q1, [x10, #0x30]
10000a3cc:     	ldr	x9, [sp, #0x140]
10000a3d0:     	str	x9, [x10, #0x50]
10000a3d4:     	ldp	q1, q0, [x11, #0x10]
10000a3d8:     	stp	q1, q0, [x10, #0x10]
10000a3dc:     	stp	x8, x28, [x10]
10000a3e0:     	b	0x10000a49c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x5bc>
10000a3e4:     	ldp	x19, x24, [sp, #0xf8]
10000a3e8:     	ldp	q0, q1, [x28, #0x20]
10000a3ec:     	stp	q0, q1, [sp, #0x80]
10000a3f0:     	ldr	q0, [x28, #0x40]
10000a3f4:     	str	q0, [sp, #0xa0]
10000a3f8:     	ldr	x22, [sp, #0x108]
10000a3fc:     	ldr	x8, [sp, #0x140]
10000a400:     	str	x8, [sp, #0xb0]
10000a404:     	mov	x25, x24
10000a408:     	mov	x21, x19
10000a40c:     	ldp	q0, q1, [sp, #0x80]
10000a410:     	ldr	x9, [sp, #0x30]
10000a414:     	stp	q0, q1, [x9, #0x20]
10000a418:     	ldr	q0, [sp, #0xa0]
10000a41c:     	str	q0, [x9, #0x40]
10000a420:     	ldr	x8, [sp, #0xb0]
10000a424:     	str	x8, [x9, #0x50]
10000a428:     	stp	x20, x21, [x9]
10000a42c:     	stp	x25, x22, [x9, #0x10]
10000a430:     	ldp	x19, x20, [sp, #0x20]
10000a434:     	b	0x10000a49c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x5bc>
10000a438:     	add	x10, sp, #0xf0
10000a43c:     	ldur	q1, [x10, #0x1c]
10000a440:     	ldr	x9, [sp, #0x30]
10000a444:     	stur	q1, [x9, #0x1c]
10000a448:     	ldur	q1, [x10, #0x2c]
10000a44c:     	stur	q1, [x9, #0x2c]
10000a450:     	ldur	q1, [x10, #0x3c]
10000a454:     	stur	q1, [x9, #0x3c]
10000a458:     	ldur	q1, [x10, #0x48]
10000a45c:     	stur	q1, [x9, #0x48]
10000a460:     	ldur	q1, [x10, #0xc]
10000a464:     	stur	q1, [x9, #0xc]
10000a468:     	str	x8, [x9]
10000a46c:     	str	s0, [x9, #0x8]
10000a470:     	b	0x10000a49c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x5bc>
10000a474:     	add	x11, sp, #0xf0
10000a478:     	ldp	q1, q2, [x11, #0x30]
10000a47c:     	ldr	x10, [sp, #0x30]
10000a480:     	stp	q1, q2, [x10, #0x30]
10000a484:     	ldr	x9, [sp, #0x140]
10000a488:     	str	x9, [x10, #0x50]
10000a48c:     	ldp	q2, q1, [x11, #0x10]
10000a490:     	stp	q2, q1, [x10, #0x10]
10000a494:     	str	x8, [x10]
10000a498:     	str	d0, [x10, #0x8]
10000a49c:     	cbz	x20, 0x10000b960 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a80>
10000a4a0:     	lsl	x1, x20, #3
10000a4a4:     	mov	x0, x19
10000a4a8:     	mov	w2, #0x8                ; =8
10000a4ac:     	bl	0x1000631dc <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10000a4b0:     	b	0x10000b960 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a80>
10000a4b4:     	tbz	w26, #0x0, 0x10000a54c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x66c>
10000a4b8:     	b.hi	0x10000b23c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x135c>
10000a4bc:     	mov	x26, x21
10000a4c0:     	cbz	x25, 0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000a4c4:     	ldur	s0, [x29, #-0xb8]
10000a4c8:     	cmp	x25, #0x10
10000a4cc:     	b.hs	0x10000b268 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1388>
10000a4d0:     	mov	x8, #0x0                ; =0
10000a4d4:     	mov	x9, x22
10000a4d8:     	b	0x10000b2b8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x13d8>
10000a4dc:     	tbz	w26, #0x0, 0x10000a570 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x690>
10000a4e0:     	b.hi	0x10000b2e0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1400>
10000a4e4:     	mov	x26, x21
10000a4e8:     	cbz	x25, 0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000a4ec:     	ldur	d0, [x29, #-0xb8]
10000a4f0:     	cmp	x25, #0x8
10000a4f4:     	b.hs	0x10000b30c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x142c>
10000a4f8:     	mov	x8, #0x0                ; =0
10000a4fc:     	mov	x9, x22
10000a500:     	b	0x10000b35c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x147c>
10000a504:     	b.hi	0x10000b384 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x14a4>
10000a508:     	mov	x26, x21
10000a50c:     	cbz	x25, 0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000a510:     	ldur	s0, [x29, #-0xb8]
10000a514:     	cmp	x25, #0x10
10000a518:     	b.hs	0x10000b3b0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x14d0>
10000a51c:     	mov	x8, #0x0                ; =0
10000a520:     	mov	x9, x22
10000a524:     	b	0x10000b400 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1520>
10000a528:     	b.hi	0x10000b428 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1548>
10000a52c:     	mov	x26, x21
10000a530:     	cbz	x25, 0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000a534:     	ldur	d0, [x29, #-0xb8]
10000a538:     	cmp	x25, #0x8
10000a53c:     	b.hs	0x10000b454 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1574>
10000a540:     	mov	x8, #0x0                ; =0
10000a544:     	mov	x9, x22
10000a548:     	b	0x10000b4a4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x15c4>
10000a54c:     	b.hi	0x10000b4cc <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x15ec>
10000a550:     	mov	x26, x21
10000a554:     	cbz	x25, 0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000a558:     	ldur	s0, [x29, #-0xb8]
10000a55c:     	cmp	x25, #0x10
10000a560:     	b.hs	0x10000b4f8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1618>
10000a564:     	mov	x8, #0x0                ; =0
10000a568:     	mov	x9, x22
10000a56c:     	b	0x10000b548 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1668>
10000a570:     	b.hi	0x10000b570 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1690>
10000a574:     	mov	x26, x21
10000a578:     	cbz	x25, 0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000a57c:     	ldur	d0, [x29, #-0xb8]
10000a580:     	cmp	x25, #0x8
10000a584:     	b.hs	0x10000b59c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x16bc>
10000a588:     	mov	x8, #0x0                ; =0
10000a58c:     	mov	x9, x22
10000a590:     	b	0x10000b5ec <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x170c>
10000a594:     	add	x0, sp, #0xc0
10000a598:     	mov	x1, #0x0                ; =0
10000a59c:     	mov	w2, #0x4                ; =4
10000a5a0:     	mov	x3, x25
10000a5a4:     	mov	w4, #0x4                ; =4
10000a5a8:     	mov	w5, #0x4                ; =4
10000a5ac:     	bl	0x100125708 <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$11finish_grow17h8de8c598a5dc063cE>
10000a5b0:     	ldr	w8, [sp, #0xc0]
10000a5b4:     	ldr	x20, [sp, #0x28]
10000a5b8:     	tbz	w8, #0x0, 0x10000af60 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1080>
10000a5bc:     	adrp	x8, 0x10012f000 <_aura_data_146+0x26a4>
10000a5c0:     	add	x8, x8, #0x201
10000a5c4:     	mov	w9, #0x17               ; =23
10000a5c8:     	stp	x8, x9, [x29, #-0x70]
10000a5cc:     	stur	x25, [x29, #-0x78]
10000a5d0:     	sub	x8, x29, #0x70
10000a5d4:     	adrp	x9, 0x100038000 <__ZN3std6thread5local17LocalKey$LT$T$GT$8try_with17hafda283f2a50e3b8E+0x58>
10000a5d8:     	add	x9, x9, #0x688
10000a5dc:     	stp	x8, x9, [sp, #0xc0]
10000a5e0:     	sub	x8, x29, #0x78
10000a5e4:     	adrp	x9, 0x100122000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
10000a5e8:     	add	x9, x9, #0xc8c
10000a5ec:     	stp	x8, x9, [sp, #0xd0]
10000a5f0:     	adrp	x0, 0x100141000 <_writev+0x100141000>
10000a5f4:     	add	x0, x0, #0xcf
10000a5f8:     	add	x8, sp, #0x50
10000a5fc:     	add	x1, sp, #0xc0
10000a600:     	bl	0x10011927c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
10000a604:     	adrp	x1, 0x10012e000 <_aura_data_146+0x16a4>
10000a608:     	add	x1, x1, #0xf53
10000a60c:     	add	x0, sp, #0xf0
10000a610:     	add	x2, sp, #0x50
10000a614:     	bl	0x10002f00c <__ZN13aura_compiler4diag10Diagnostic5coded17hc0c6d1b6375dce8eE>
10000a618:     	ldp	x20, x19, [sp, #0xf0]
10000a61c:     	ldp	x24, x22, [sp, #0x100]
10000a620:     	cmp	x20, #0x2
10000a624:     	b.ne	0x10000af70 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1090>
10000a628:     	add	x28, sp, #0xf0
10000a62c:     	stp	x19, x24, [x29, #-0xb0]
10000a630:     	stur	x22, [x29, #-0xa0]
10000a634:     	cmp	x23, #0x1
10000a638:     	b.le	0x10000a290 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x3b0>
10000a63c:     	cmp	x23, #0x2
10000a640:     	b.ne	0x10000a674 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x794>
10000a644:     	sub	x8, x19, x22
10000a648:     	cmp	x25, x8
10000a64c:     	tbz	w26, #0x0, 0x10000a6c4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x7e4>
10000a650:     	b.hi	0x10000b614 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1734>
10000a654:     	mov	x26, x21
10000a658:     	cbz	x25, 0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000a65c:     	ldur	s0, [x29, #-0xb8]
10000a660:     	cmp	x25, #0x10
10000a664:     	b.hs	0x10000b640 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1760>
10000a668:     	mov	x8, #0x0                ; =0
10000a66c:     	mov	x9, x22
10000a670:     	b	0x10000b68c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x17ac>
10000a674:     	tbz	w26, #0x0, 0x10000a6e8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x808>
10000a678:     	lsl	x9, x25, #2
10000a67c:     	mov	x26, x21
10000a680:     	cbz	x25, 0x10000a6a4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x7c4>
10000a684:     	mov	x8, #0x0                ; =0
10000a688:     	mov	x10, x9
10000a68c:     	ldr	s0, [x27, x8, lsl #2]
10000a690:     	fcmp	s0, #0.0
10000a694:     	b.eq	0x10000a700 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x820>
10000a698:     	add	x8, x8, #0x1
10000a69c:     	subs	x10, x10, #0x4
10000a6a0:     	b.ne	0x10000a68c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x7ac>
10000a6a4:     	add	x8, x27, x9
10000a6a8:     	stp	x27, x8, [x29, #-0xd8]
10000a6ac:     	sub	x8, x29, #0xb8
10000a6b0:     	stur	x8, [x29, #-0xc8]
10000a6b4:     	sub	x0, x29, #0xb0
10000a6b8:     	sub	x1, x29, #0xd8
10000a6bc:     	bl	0x10004ed68 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hfd737092eb9c7864E>
10000a6c0:     	b	0x10000b754 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1874>
10000a6c4:     	b.hi	0x10000b6b4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x17d4>
10000a6c8:     	mov	x26, x21
10000a6cc:     	cbz	x25, 0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000a6d0:     	ldur	s0, [x29, #-0xb8]
10000a6d4:     	cmp	x25, #0x10
10000a6d8:     	b.hs	0x10000b6e0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1800>
10000a6dc:     	mov	x8, #0x0                ; =0
10000a6e0:     	mov	x9, x22
10000a6e4:     	b	0x10000b72c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x184c>
10000a6e8:     	cbz	x25, 0x10000a7b4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x8d4>
10000a6ec:     	ldur	s0, [x29, #-0xb8]
10000a6f0:     	fcmp	s0, #0.0
10000a6f4:     	b.ne	0x10000a7b4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x8d4>
10000a6f8:     	mov	x8, #0x0                ; =0
10000a6fc:     	mov	x26, x21
10000a700:     	stur	x8, [x29, #-0x78]
10000a704:     	adrp	x8, 0x10012f000 <_aura_data_146+0x26a4>
10000a708:     	add	x8, x8, #0x21e
10000a70c:     	mov	w9, #0x12               ; =18
10000a710:     	stp	x8, x9, [x29, #-0x70]
10000a714:     	adrp	x8, 0x100130000 <_aura_data_146+0x36a4>
10000a718:     	add	x8, x8, #0x998
10000a71c:     	mov	w9, #0x8                ; =8
10000a720:     	stp	x8, x9, [sp, #0x50]
10000a724:     	add	x8, sp, #0x50
10000a728:     	adrp	x9, 0x100038000 <__ZN3std6thread5local17LocalKey$LT$T$GT$8try_with17hafda283f2a50e3b8E+0x58>
10000a72c:     	add	x9, x9, #0x688
10000a730:     	stp	x8, x9, [sp, #0xf0]
10000a734:     	sub	x8, x29, #0x70
10000a738:     	stp	x8, x9, [sp, #0x100]
10000a73c:     	sub	x8, x29, #0x78
10000a740:     	adrp	x9, 0x100122000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
10000a744:     	add	x9, x9, #0xc8c
10000a748:     	stp	x8, x9, [sp, #0x110]
10000a74c:     	adrp	x0, 0x100141000 <_writev+0x100141000>
10000a750:     	add	x0, x0, #0x103
10000a754:     	add	x8, sp, #0xc0
10000a758:     	add	x1, sp, #0xf0
10000a75c:     	bl	0x10011927c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
10000a760:     	adrp	x1, 0x10012f000 <_aura_data_146+0x26a4>
10000a764:     	add	x1, x1, #0x218
10000a768:     	add	x0, sp, #0xf0
10000a76c:     	add	x2, sp, #0xc0
10000a770:     	bl	0x10002f00c <__ZN13aura_compiler4diag10Diagnostic5coded17hc0c6d1b6375dce8eE>
10000a774:     	ldp	x20, x21, [sp, #0xf0]
10000a778:     	ldp	x25, x22, [sp, #0x100]
10000a77c:     	ldp	q0, q1, [x28, #0x20]
10000a780:     	stp	q0, q1, [sp, #0x80]
10000a784:     	ldr	q0, [x28, #0x40]
10000a788:     	str	q0, [sp, #0xa0]
10000a78c:     	ldr	x8, [sp, #0x140]
10000a790:     	str	x8, [sp, #0xb0]
10000a794:     	cbz	x19, 0x10000a7a8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x8c8>
10000a798:     	lsl	x1, x19, #2
10000a79c:     	mov	x0, x24
10000a7a0:     	mov	w2, #0x4                ; =4
10000a7a4:     	bl	0x1000631dc <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10000a7a8:     	cmp	x20, #0x2
10000a7ac:     	b.ne	0x10000a40c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x52c>
10000a7b0:     	b	0x10000b75c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x187c>
10000a7b4:     	add	x8, x27, x25, lsl #2
10000a7b8:     	stp	x27, x8, [x29, #-0x90]
10000a7bc:     	sub	x8, x29, #0xb8
10000a7c0:     	stur	x8, [x29, #-0x80]
10000a7c4:     	sub	x0, x29, #0xb0
10000a7c8:     	sub	x1, x29, #0x90
10000a7cc:     	bl	0x10004e9d8 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h42310c836ddc5743E>
10000a7d0:     	mov	x26, x21
10000a7d4:     	b	0x10000b754 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1874>
10000a7d8:     	add	x0, sp, #0xc0
10000a7dc:     	mov	x1, #0x0                ; =0
10000a7e0:     	mov	w2, #0x8                ; =8
10000a7e4:     	mov	x3, x25
10000a7e8:     	mov	w4, #0x8                ; =8
10000a7ec:     	mov	w5, #0x8                ; =8
10000a7f0:     	bl	0x100125708 <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$11finish_grow17h8de8c598a5dc063cE>
10000a7f4:     	ldr	w8, [sp, #0xc0]
10000a7f8:     	ldr	x20, [sp, #0x28]
10000a7fc:     	tbz	w8, #0x0, 0x10000af68 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1088>
10000a800:     	adrp	x8, 0x10012f000 <_aura_data_146+0x26a4>
10000a804:     	add	x8, x8, #0x201
10000a808:     	mov	w9, #0x17               ; =23
10000a80c:     	stp	x8, x9, [x29, #-0x70]
10000a810:     	stur	x25, [x29, #-0x78]
10000a814:     	sub	x8, x29, #0x70
10000a818:     	adrp	x9, 0x100038000 <__ZN3std6thread5local17LocalKey$LT$T$GT$8try_with17hafda283f2a50e3b8E+0x58>
10000a81c:     	add	x9, x9, #0x688
10000a820:     	stp	x8, x9, [sp, #0xc0]
10000a824:     	sub	x8, x29, #0x78
10000a828:     	adrp	x9, 0x100122000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
10000a82c:     	add	x9, x9, #0xc8c
10000a830:     	stp	x8, x9, [sp, #0xd0]
10000a834:     	adrp	x0, 0x100141000 <_writev+0x100141000>
10000a838:     	add	x0, x0, #0xcf
10000a83c:     	add	x8, sp, #0x50
10000a840:     	add	x1, sp, #0xc0
10000a844:     	bl	0x10011927c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
10000a848:     	adrp	x1, 0x10012e000 <_aura_data_146+0x16a4>
10000a84c:     	add	x1, x1, #0xf53
10000a850:     	add	x0, sp, #0xf0
10000a854:     	add	x2, sp, #0x50
10000a858:     	bl	0x10002f00c <__ZN13aura_compiler4diag10Diagnostic5coded17hc0c6d1b6375dce8eE>
10000a85c:     	ldp	x20, x19, [sp, #0xf0]
10000a860:     	ldp	x24, x22, [sp, #0x100]
10000a864:     	cmp	x20, #0x2
10000a868:     	b.ne	0x10000af70 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1090>
10000a86c:     	add	x28, sp, #0xf0
10000a870:     	stp	x19, x24, [x29, #-0xb0]
10000a874:     	stur	x22, [x29, #-0xa0]
10000a878:     	cmp	x23, #0x1
10000a87c:     	b.le	0x10000a34c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x46c>
10000a880:     	cmp	x23, #0x2
10000a884:     	b.ne	0x10000a8b8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x9d8>
10000a888:     	sub	x8, x19, x22
10000a88c:     	cmp	x25, x8
10000a890:     	tbz	w26, #0x0, 0x10000a908 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xa28>
10000a894:     	b.hi	0x10000b7b4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x18d4>
10000a898:     	mov	x26, x21
10000a89c:     	cbz	x25, 0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000a8a0:     	ldur	d0, [x29, #-0xb8]
10000a8a4:     	cmp	x25, #0x8
10000a8a8:     	b.hs	0x10000b7e0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1900>
10000a8ac:     	mov	x8, #0x0                ; =0
10000a8b0:     	mov	x9, x22
10000a8b4:     	b	0x10000b82c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x194c>
10000a8b8:     	tbz	w26, #0x0, 0x10000a92c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xa4c>
10000a8bc:     	lsl	x9, x25, #3
10000a8c0:     	mov	x26, x21
10000a8c4:     	cbz	x25, 0x10000a8e8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xa08>
10000a8c8:     	mov	x8, #0x0                ; =0
10000a8cc:     	mov	x10, x9
10000a8d0:     	ldr	d0, [x27, x8, lsl #3]
10000a8d4:     	fcmp	d0, #0.0
10000a8d8:     	b.eq	0x10000a944 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xa64>
10000a8dc:     	add	x8, x8, #0x1
10000a8e0:     	subs	x10, x10, #0x8
10000a8e4:     	b.ne	0x10000a8d0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x9f0>
10000a8e8:     	add	x8, x27, x9
10000a8ec:     	stp	x27, x8, [x29, #-0xd8]
10000a8f0:     	sub	x8, x29, #0xb8
10000a8f4:     	stur	x8, [x29, #-0xc8]
10000a8f8:     	sub	x0, x29, #0xb0
10000a8fc:     	sub	x1, x29, #0xd8
10000a900:     	bl	0x10004ec38 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hc4092ea7f1480ea8E>
10000a904:     	b	0x10000b8f4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a14>
10000a908:     	b.hi	0x10000b854 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1974>
10000a90c:     	mov	x26, x21
10000a910:     	cbz	x25, 0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000a914:     	ldur	d0, [x29, #-0xb8]
10000a918:     	cmp	x25, #0x8
10000a91c:     	b.hs	0x10000b880 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x19a0>
10000a920:     	mov	x8, #0x0                ; =0
10000a924:     	mov	x9, x22
10000a928:     	b	0x10000b8cc <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x19ec>
10000a92c:     	cbz	x25, 0x10000a9f8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xb18>
10000a930:     	ldur	d0, [x29, #-0xb8]
10000a934:     	fcmp	d0, #0.0
10000a938:     	b.ne	0x10000a9f8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xb18>
10000a93c:     	mov	x8, #0x0                ; =0
10000a940:     	mov	x26, x21
10000a944:     	stur	x8, [x29, #-0x78]
10000a948:     	adrp	x8, 0x10012f000 <_aura_data_146+0x26a4>
10000a94c:     	add	x8, x8, #0x21e
10000a950:     	mov	w9, #0x12               ; =18
10000a954:     	stp	x8, x9, [x29, #-0x70]
10000a958:     	adrp	x8, 0x100130000 <_aura_data_146+0x36a4>
10000a95c:     	add	x8, x8, #0x998
10000a960:     	mov	w9, #0x8                ; =8
10000a964:     	stp	x8, x9, [sp, #0x50]
10000a968:     	add	x8, sp, #0x50
10000a96c:     	adrp	x9, 0x100038000 <__ZN3std6thread5local17LocalKey$LT$T$GT$8try_with17hafda283f2a50e3b8E+0x58>
10000a970:     	add	x9, x9, #0x688
10000a974:     	stp	x8, x9, [sp, #0xf0]
10000a978:     	sub	x8, x29, #0x70
10000a97c:     	stp	x8, x9, [sp, #0x100]
10000a980:     	sub	x8, x29, #0x78
10000a984:     	adrp	x9, 0x100122000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
10000a988:     	add	x9, x9, #0xc8c
10000a98c:     	stp	x8, x9, [sp, #0x110]
10000a990:     	adrp	x0, 0x100141000 <_writev+0x100141000>
10000a994:     	add	x0, x0, #0x103
10000a998:     	add	x8, sp, #0xc0
10000a99c:     	add	x1, sp, #0xf0
10000a9a0:     	bl	0x10011927c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
10000a9a4:     	adrp	x1, 0x10012f000 <_aura_data_146+0x26a4>
10000a9a8:     	add	x1, x1, #0x218
10000a9ac:     	add	x0, sp, #0xf0
10000a9b0:     	add	x2, sp, #0xc0
10000a9b4:     	bl	0x10002f00c <__ZN13aura_compiler4diag10Diagnostic5coded17hc0c6d1b6375dce8eE>
10000a9b8:     	ldp	x20, x21, [sp, #0xf0]
10000a9bc:     	ldp	x25, x22, [sp, #0x100]
10000a9c0:     	ldp	q0, q1, [x28, #0x20]
10000a9c4:     	stp	q0, q1, [sp, #0x80]
10000a9c8:     	ldr	q0, [x28, #0x40]
10000a9cc:     	str	q0, [sp, #0xa0]
10000a9d0:     	ldr	x8, [sp, #0x140]
10000a9d4:     	str	x8, [sp, #0xb0]
10000a9d8:     	cbz	x19, 0x10000a9ec <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xb0c>
10000a9dc:     	lsl	x1, x19, #3
10000a9e0:     	mov	x0, x24
10000a9e4:     	mov	w2, #0x8                ; =8
10000a9e8:     	bl	0x1000631dc <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10000a9ec:     	cmp	x20, #0x2
10000a9f0:     	b.ne	0x10000a40c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x52c>
10000a9f4:     	b	0x10000b8fc <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a1c>
10000a9f8:     	add	x8, x27, x25, lsl #3
10000a9fc:     	stp	x27, x8, [x29, #-0x90]
10000aa00:     	sub	x8, x29, #0xb8
10000aa04:     	stur	x8, [x29, #-0x80]
10000aa08:     	sub	x0, x29, #0xb0
10000aa0c:     	sub	x1, x29, #0x90
10000aa10:     	bl	0x10004eb08 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h8cdd759e6d87109bE>
10000aa14:     	mov	x26, x21
10000aa18:     	b	0x10000b8f4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a14>
10000aa1c:     	sub	x0, x29, #0xd8
10000aa20:     	mov	x1, #0x0                ; =0
10000aa24:     	mov	w2, #0x4                ; =4
10000aa28:     	mov	x3, x25
10000aa2c:     	mov	w4, #0x4                ; =4
10000aa30:     	mov	w5, #0x4                ; =4
10000aa34:     	bl	0x100125708 <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$11finish_grow17h8de8c598a5dc063cE>
10000aa38:     	ldur	w8, [x29, #-0xd8]
10000aa3c:     	tbz	w8, #0x0, 0x10000af88 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x10a8>
10000aa40:     	adrp	x8, 0x10012f000 <_aura_data_146+0x26a4>
10000aa44:     	add	x8, x8, #0x201
10000aa48:     	mov	w9, #0x17               ; =23
10000aa4c:     	stp	x8, x9, [x29, #-0x70]
10000aa50:     	stur	x25, [x29, #-0x78]
10000aa54:     	sub	x8, x29, #0x70
10000aa58:     	adrp	x9, 0x100038000 <__ZN3std6thread5local17LocalKey$LT$T$GT$8try_with17hafda283f2a50e3b8E+0x58>
10000aa5c:     	add	x9, x9, #0x688
10000aa60:     	stp	x8, x9, [x29, #-0xd8]
10000aa64:     	sub	x8, x29, #0x78
10000aa68:     	adrp	x9, 0x100122000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
10000aa6c:     	add	x9, x9, #0xc8c
10000aa70:     	stp	x8, x9, [x29, #-0xc8]
10000aa74:     	adrp	x0, 0x100141000 <_writev+0x100141000>
10000aa78:     	add	x0, x0, #0xcf
10000aa7c:     	sub	x8, x29, #0x90
10000aa80:     	sub	x1, x29, #0xd8
10000aa84:     	bl	0x10011927c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
10000aa88:     	adrp	x1, 0x10012e000 <_aura_data_146+0x16a4>
10000aa8c:     	add	x1, x1, #0xf53
10000aa90:     	add	x0, sp, #0xf0
10000aa94:     	sub	x2, x29, #0x90
10000aa98:     	bl	0x10002f00c <__ZN13aura_compiler4diag10Diagnostic5coded17hc0c6d1b6375dce8eE>
10000aa9c:     	ldp	x19, x8, [sp, #0xf0]
10000aaa0:     	ldp	x20, x22, [sp, #0x100]
10000aaa4:     	cmp	x19, #0x2
10000aaa8:     	b.ne	0x10000b060 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1180>
10000aaac:     	stp	x8, x20, [x29, #-0xb0]
10000aab0:     	stur	x22, [x29, #-0xa0]
10000aab4:     	lsl	x19, x25, #2
10000aab8:     	str	x21, [sp, #0x10]
10000aabc:     	mov	x25, #0x0               ; =0
10000aac0:     	tbnz	w26, #0x0, 0x100009fac <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xcc>
10000aac4:     	sxtw	x26, w21
10000aac8:     	asr	x28, x26, #63
10000aacc:     	mov	x27, #-0x1              ; =-1
10000aad0:     	b	0x10000aaf0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xc10>
10000aad4:     	ldur	x20, [x29, #-0xa8]
10000aad8:     	str	w21, [x20, x22, lsl #2]
10000aadc:     	add	x22, x22, #0x1
10000aae0:     	stur	x22, [x29, #-0xa0]
10000aae4:     	add	x25, x25, #0x1
10000aae8:     	subs	x19, x19, #0x4
10000aaec:     	b.eq	0x10000af9c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x10bc>
10000aaf0:     	ldp	x9, x8, [sp, #0x10]
10000aaf4:     	ldrsw	x8, [x8, x25, lsl #2]
10000aaf8:     	mvn	w9, w9
10000aafc:     	mvn	w10, w8
10000ab00:     	lsr	w10, w10, #31
10000ab04:     	stp	x10, xzr, [sp, #0xc0]
10000ab08:     	lsr	w9, w9, #31
10000ab0c:     	asr	x10, x8, #63
10000ab10:     	stp	x8, x10, [sp, #0xd0]
10000ab14:     	mov	w8, #0x2                ; =2
10000ab18:     	strb	w8, [sp, #0xe0]
10000ab1c:     	stp	x9, xzr, [sp, #0x80]
10000ab20:     	stp	x26, x28, [sp, #0x90]
10000ab24:     	strb	w8, [sp, #0xa0]
10000ab28:     	add	x0, sp, #0xf0
10000ab2c:     	add	x1, sp, #0xc0
10000ab30:     	add	x2, sp, #0x80
10000ab34:     	mov	x3, x23
10000ab38:     	mov	x4, x24
10000ab3c:     	mov	x5, x25
10000ab40:     	bl	0x100017ce0 <__ZN13aura_compiler13runtime_value29apply_integer_array_operation17h418583a9cdd5b1bcE>
10000ab44:     	ldr	x8, [sp, #0xf0]
10000ab48:     	cmp	x8, #0x1
10000ab4c:     	b.eq	0x10000ab98 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xcb8>
10000ab50:     	ldr	w9, [sp, #0x100]
10000ab54:     	ldp	x21, x8, [sp, #0x110]
10000ab58:     	tbz	w9, #0x0, 0x10000ab60 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xc80>
10000ab5c:     	tbnz	x8, #0x3f, 0x10000b0bc <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x11dc>
10000ab60:     	mov	x9, #-0x80000000        ; =-2147483648
10000ab64:     	adds	x9, x21, x9
10000ab68:     	adc	x8, x8, x27
10000ab6c:     	mov	x10, #-0x100000001      ; =-4294967297
10000ab70:     	cmp	x10, x9
10000ab74:     	sbcs	xzr, x27, x8
10000ab78:     	b.hs	0x10000abcc <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xcec>
10000ab7c:     	ldur	x22, [x29, #-0xa0]
10000ab80:     	ldur	x8, [x29, #-0xb0]
10000ab84:     	cmp	x22, x8
10000ab88:     	b.ne	0x10000aad8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xbf8>
10000ab8c:     	sub	x0, x29, #0xb0
10000ab90:     	bl	0x100125360 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hcd4bc81e77a6b15aE>
10000ab94:     	b	0x10000aad4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xbf4>
10000ab98:     	add	x8, sp, #0xf0
10000ab9c:     	ldp	x19, x26, [sp, #0xf8]
10000aba0:     	ldp	x22, x23, [sp, #0x110]
10000aba4:     	ldr	x9, [sp, #0x108]
10000aba8:     	ldrb	w24, [sp, #0x120]
10000abac:     	ldur	q0, [x8, #0x31]
10000abb0:     	ldur	q1, [x8, #0x41]
10000abb4:     	stp	q0, q1, [sp, #0x50]
10000abb8:     	ldur	q0, [x8, #0x50]
10000abbc:     	stur	q0, [sp, #0x6f]
10000abc0:     	extr	x25, x9, x26, #0x20
10000abc4:     	lsr	x20, x9, #32
10000abc8:     	b	0x10000ac7c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xd9c>
10000abcc:     	adrp	x8, 0x10012f000 <_aura_data_146+0x26a4>
10000abd0:     	add	x8, x8, #0x256
10000abd4:     	adrp	x9, 0x100161000 <_writev+0x100161000>
10000abd8:     	add	x9, x9, #0x638
10000abdc:     	mov	w10, #0xa               ; =10
10000abe0:     	ldr	x9, [x9, x23, lsl #3]
10000abe4:     	adrp	x11, 0x100130000 <_aura_data_146+0x36a4>
10000abe8:     	add	x11, x11, #0x638
10000abec:     	stp	x25, x8, [x29, #-0x78]
10000abf0:     	stur	x10, [x29, #-0x68]
10000abf4:     	ldr	x8, [x11, x23, lsl #3]
10000abf8:     	stp	x9, x8, [x29, #-0x90]
10000abfc:     	sub	x8, x29, #0x90
10000ac00:     	adrp	x9, 0x100038000 <__ZN3std6thread5local17LocalKey$LT$T$GT$8try_with17hafda283f2a50e3b8E+0x58>
10000ac04:     	add	x9, x9, #0x688
10000ac08:     	stp	x8, x9, [sp, #0xf0]
10000ac0c:     	sub	x8, x29, #0x70
10000ac10:     	stp	x8, x9, [sp, #0x100]
10000ac14:     	sub	x8, x29, #0x78
10000ac18:     	adrp	x9, 0x100122000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
10000ac1c:     	add	x9, x9, #0xc8c
10000ac20:     	stp	x8, x9, [sp, #0x110]
10000ac24:     	adrp	x0, 0x100141000 <_writev+0x100141000>
10000ac28:     	add	x0, x0, #0x103
10000ac2c:     	sub	x8, x29, #0xd8
10000ac30:     	add	x1, sp, #0xf0
10000ac34:     	bl	0x10011927c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
10000ac38:     	add	x21, sp, #0xf0
10000ac3c:     	adrp	x1, 0x10012f000 <_aura_data_146+0x26a4>
10000ac40:     	add	x1, x1, #0x6c
10000ac44:     	add	x0, sp, #0xf0
10000ac48:     	sub	x2, x29, #0xd8
10000ac4c:     	bl	0x10002f00c <__ZN13aura_compiler4diag10Diagnostic5coded17hc0c6d1b6375dce8eE>
10000ac50:     	ldr	x19, [sp, #0xf0]
10000ac54:     	ldr	w26, [sp, #0xf8]
10000ac58:     	ldr	w20, [sp, #0x104]
10000ac5c:     	ldur	x25, [x21, #0xc]
10000ac60:     	ldp	x22, x23, [sp, #0x108]
10000ac64:     	ldrb	w24, [sp, #0x118]
10000ac68:     	ldur	q0, [x21, #0x29]
10000ac6c:     	ldur	q1, [x21, #0x39]
10000ac70:     	stp	q0, q1, [sp, #0x50]
10000ac74:     	ldur	q0, [x21, #0x48]
10000ac78:     	stur	q0, [sp, #0x6f]
10000ac7c:     	ldur	x8, [x29, #-0xb0]
10000ac80:     	cbz	x8, 0x10000ac94 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xdb4>
10000ac84:     	ldur	x0, [x29, #-0xa8]
10000ac88:     	lsl	x1, x8, #2
10000ac8c:     	mov	w2, #0x4                ; =4
10000ac90:     	bl	0x1000631dc <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10000ac94:     	cmp	x19, #0x2
10000ac98:     	b.eq	0x10000afa8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x10c8>
10000ac9c:     	ldp	q0, q1, [sp, #0x50]
10000aca0:     	ldr	x10, [sp, #0x30]
10000aca4:     	stur	q0, [x10, #0x29]
10000aca8:     	mov	w8, w26
10000acac:     	orr	x8, x8, x25, lsl #32
10000acb0:     	stur	q1, [x10, #0x39]
10000acb4:     	ldur	q0, [sp, #0x6f]
10000acb8:     	stur	q0, [x10, #0x48]
10000acbc:     	extr	x9, x20, x25, #0x20
10000acc0:     	stp	x19, x8, [x10]
10000acc4:     	stp	x9, x22, [x10, #0x10]
10000acc8:     	str	x23, [x10, #0x20]
10000accc:     	strb	w24, [x10, #0x28]
10000acd0:     	b	0x10000a430 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x550>
10000acd4:     	sub	x0, x29, #0xd8
10000acd8:     	mov	x1, #0x0                ; =0
10000acdc:     	mov	w2, #0x8                ; =8
10000ace0:     	mov	x3, x27
10000ace4:     	mov	w4, #0x8                ; =8
10000ace8:     	mov	w5, #0x8                ; =8
10000acec:     	bl	0x100125708 <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$11finish_grow17h8de8c598a5dc063cE>
10000acf0:     	ldur	w8, [x29, #-0xd8]
10000acf4:     	tbz	w8, #0x0, 0x10000b004 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1124>
10000acf8:     	adrp	x8, 0x10012f000 <_aura_data_146+0x26a4>
10000acfc:     	add	x8, x8, #0x201
10000ad00:     	mov	w9, #0x17               ; =23
10000ad04:     	stp	x8, x9, [x29, #-0x70]
10000ad08:     	stur	x27, [x29, #-0x78]
10000ad0c:     	sub	x8, x29, #0x70
10000ad10:     	adrp	x9, 0x100038000 <__ZN3std6thread5local17LocalKey$LT$T$GT$8try_with17hafda283f2a50e3b8E+0x58>
10000ad14:     	add	x9, x9, #0x688
10000ad18:     	stp	x8, x9, [x29, #-0xd8]
10000ad1c:     	sub	x8, x29, #0x78
10000ad20:     	adrp	x9, 0x100122000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
10000ad24:     	add	x9, x9, #0xc8c
10000ad28:     	stp	x8, x9, [x29, #-0xc8]
10000ad2c:     	adrp	x0, 0x100141000 <_writev+0x100141000>
10000ad30:     	add	x0, x0, #0xcf
10000ad34:     	sub	x8, x29, #0x90
10000ad38:     	sub	x1, x29, #0xd8
10000ad3c:     	bl	0x10011927c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
10000ad40:     	adrp	x1, 0x10012e000 <_aura_data_146+0x16a4>
10000ad44:     	add	x1, x1, #0xf53
10000ad48:     	add	x0, sp, #0xf0
10000ad4c:     	sub	x2, x29, #0x90
10000ad50:     	bl	0x10002f00c <__ZN13aura_compiler4diag10Diagnostic5coded17hc0c6d1b6375dce8eE>
10000ad54:     	ldp	x8, x19, [sp, #0xf0]
10000ad58:     	ldp	x25, x22, [sp, #0x100]
10000ad5c:     	cmp	x8, #0x2
10000ad60:     	b.ne	0x10000b090 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x11b0>
10000ad64:     	stp	x19, x25, [x29, #-0xb0]
10000ad68:     	stur	x22, [x29, #-0xa0]
10000ad6c:     	lsl	x19, x27, #3
10000ad70:     	tbnz	w26, #0x0, 0x10000a05c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x17c>
10000ad74:     	mov	x26, #0x0               ; =0
10000ad78:     	lsr	x8, x28, #63
10000ad7c:     	eor	w27, w8, #0x1
10000ad80:     	asr	x20, x28, #63
10000ad84:     	b	0x10000ada4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xec4>
10000ad88:     	ldur	x25, [x29, #-0xa8]
10000ad8c:     	str	x22, [x25, x21, lsl #3]
10000ad90:     	add	x22, x21, #0x1
10000ad94:     	stur	x22, [x29, #-0xa0]
10000ad98:     	add	x26, x26, #0x1
10000ad9c:     	subs	x19, x19, #0x8
10000ada0:     	b.eq	0x10000b018 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1138>
10000ada4:     	ldr	x8, [sp, #0x18]
10000ada8:     	ldr	x8, [x8, x26, lsl #3]
10000adac:     	lsr	x9, x8, #63
10000adb0:     	eor	w9, w9, #0x1
10000adb4:     	stp	x9, xzr, [sp, #0xc0]
10000adb8:     	asr	x9, x8, #63
10000adbc:     	stp	x8, x9, [sp, #0xd0]
10000adc0:     	mov	w8, #0x3                ; =3
10000adc4:     	strb	w8, [sp, #0xe0]
10000adc8:     	stp	x27, xzr, [sp, #0x80]
10000adcc:     	stp	x28, x20, [sp, #0x90]
10000add0:     	strb	w8, [sp, #0xa0]
10000add4:     	add	x0, sp, #0xf0
10000add8:     	add	x1, sp, #0xc0
10000addc:     	add	x2, sp, #0x80
10000ade0:     	mov	x3, x23
10000ade4:     	mov	x4, x24
10000ade8:     	mov	x5, x26
10000adec:     	bl	0x100017ce0 <__ZN13aura_compiler13runtime_value29apply_integer_array_operation17h418583a9cdd5b1bcE>
10000adf0:     	ldr	x8, [sp, #0xf0]
10000adf4:     	cmp	x8, #0x1
10000adf8:     	b.eq	0x10000ae40 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xf60>
10000adfc:     	ldr	w9, [sp, #0x100]
10000ae00:     	ldp	x22, x8, [sp, #0x110]
10000ae04:     	tbz	w9, #0x0, 0x10000ae0c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xf2c>
10000ae08:     	tbnz	x8, #0x3f, 0x10000b0d8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x11f8>
10000ae0c:     	mov	x9, #-0x8000000000000000 ; =-9223372036854775808
10000ae10:     	cmn	x22, x9
10000ae14:     	mov	x9, #-0x1               ; =-1
10000ae18:     	adc	x8, x8, x9
10000ae1c:     	cmn	x8, #0x1
10000ae20:     	b.ne	0x10000ae68 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xf88>
10000ae24:     	ldur	x21, [x29, #-0xa0]
10000ae28:     	ldur	x8, [x29, #-0xb0]
10000ae2c:     	cmp	x21, x8
10000ae30:     	b.ne	0x10000ad8c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xeac>
10000ae34:     	sub	x0, x29, #0xb0
10000ae38:     	bl	0x1001253c8 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hf9c8210e3d80a166E>
10000ae3c:     	b	0x10000ad88 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xea8>
10000ae40:     	add	x8, sp, #0xf0
10000ae44:     	ldp	x24, x19, [sp, #0xf8]
10000ae48:     	ldp	x22, x20, [sp, #0x110]
10000ae4c:     	ldr	x25, [sp, #0x108]
10000ae50:     	ldrb	w23, [sp, #0x120]
10000ae54:     	ldur	q0, [x8, #0x31]
10000ae58:     	ldur	q1, [x8, #0x41]
10000ae5c:     	stp	q0, q1, [sp, #0x50]
10000ae60:     	ldur	q0, [x8, #0x50]
10000ae64:     	b	0x10000af0c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x102c>
10000ae68:     	adrp	x8, 0x10012f000 <_aura_data_146+0x26a4>
10000ae6c:     	add	x8, x8, #0x256
10000ae70:     	adrp	x9, 0x100161000 <_writev+0x100161000>
10000ae74:     	add	x9, x9, #0x638
10000ae78:     	mov	w10, #0xa               ; =10
10000ae7c:     	ldr	x9, [x9, x23, lsl #3]
10000ae80:     	adrp	x11, 0x100130000 <_aura_data_146+0x36a4>
10000ae84:     	add	x11, x11, #0x638
10000ae88:     	stp	x26, x8, [x29, #-0x78]
10000ae8c:     	stur	x10, [x29, #-0x68]
10000ae90:     	ldr	x8, [x11, x23, lsl #3]
10000ae94:     	stp	x9, x8, [x29, #-0x90]
10000ae98:     	sub	x8, x29, #0x90
10000ae9c:     	adrp	x9, 0x100038000 <__ZN3std6thread5local17LocalKey$LT$T$GT$8try_with17hafda283f2a50e3b8E+0x58>
10000aea0:     	add	x9, x9, #0x688
10000aea4:     	stp	x8, x9, [sp, #0xf0]
10000aea8:     	sub	x8, x29, #0x70
10000aeac:     	stp	x8, x9, [sp, #0x100]
10000aeb0:     	sub	x8, x29, #0x78
10000aeb4:     	adrp	x9, 0x100122000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
10000aeb8:     	add	x9, x9, #0xc8c
10000aebc:     	stp	x8, x9, [sp, #0x110]
10000aec0:     	adrp	x0, 0x100141000 <_writev+0x100141000>
10000aec4:     	add	x0, x0, #0x103
10000aec8:     	sub	x8, x29, #0xd8
10000aecc:     	add	x1, sp, #0xf0
10000aed0:     	bl	0x10011927c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
10000aed4:     	add	x21, sp, #0xf0
10000aed8:     	adrp	x1, 0x10012f000 <_aura_data_146+0x26a4>
10000aedc:     	add	x1, x1, #0x6c
10000aee0:     	add	x0, sp, #0xf0
10000aee4:     	sub	x2, x29, #0xd8
10000aee8:     	bl	0x10002f00c <__ZN13aura_compiler4diag10Diagnostic5coded17hc0c6d1b6375dce8eE>
10000aeec:     	ldp	x24, x19, [sp, #0xf0]
10000aef0:     	ldp	x22, x20, [sp, #0x108]
10000aef4:     	ldr	x25, [sp, #0x100]
10000aef8:     	ldrb	w23, [sp, #0x118]
10000aefc:     	ldur	q0, [x21, #0x29]
10000af00:     	ldur	q1, [x21, #0x39]
10000af04:     	stp	q0, q1, [sp, #0x50]
10000af08:     	ldur	q0, [x21, #0x48]
10000af0c:     	stur	q0, [sp, #0x6f]
10000af10:     	ldr	x26, [sp, #0x8]
10000af14:     	ldur	x8, [x29, #-0xb0]
10000af18:     	cbz	x8, 0x10000af2c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x104c>
10000af1c:     	ldur	x0, [x29, #-0xa8]
10000af20:     	lsl	x1, x8, #3
10000af24:     	mov	w2, #0x8                ; =8
10000af28:     	bl	0x1000631dc <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10000af2c:     	cmp	x24, #0x2
10000af30:     	b.eq	0x10000b020 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1140>
10000af34:     	ldp	q0, q1, [sp, #0x50]
10000af38:     	ldr	x8, [sp, #0x30]
10000af3c:     	stur	q0, [x8, #0x29]
10000af40:     	stur	q1, [x8, #0x39]
10000af44:     	ldur	q0, [sp, #0x6f]
10000af48:     	stur	q0, [x8, #0x48]
10000af4c:     	stp	x24, x19, [x8]
10000af50:     	stp	x25, x22, [x8, #0x10]
10000af54:     	str	x20, [x8, #0x20]
10000af58:     	strb	w23, [x8, #0x28]
10000af5c:     	b	0x10000a430 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x550>
10000af60:     	ldr	x24, [sp, #0xc8]
10000af64:     	b	0x10000a274 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x394>
10000af68:     	ldr	x24, [sp, #0xc8]
10000af6c:     	b	0x10000a330 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x450>
10000af70:     	add	x8, sp, #0xf0
10000af74:     	ldp	q0, q1, [x8, #0x20]
10000af78:     	stp	q0, q1, [sp, #0x80]
10000af7c:     	ldr	q0, [x8, #0x40]
10000af80:     	str	q0, [sp, #0xa0]
10000af84:     	b	0x10000a3fc <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x51c>
10000af88:     	ldur	x20, [x29, #-0xd0]
10000af8c:     	stp	x25, x20, [x29, #-0xb0]
10000af90:     	stur	xzr, [x29, #-0xa0]
10000af94:     	cbnz	x25, 0x100009f9c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xbc>
10000af98:     	mov	x22, #0x0               ; =0
10000af9c:     	ldp	x26, x8, [x29, #-0xb0]
10000afa0:     	extr	x25, x8, x26, #0x20
10000afa4:     	lsr	x20, x8, #32
10000afa8:     	mov	w8, w26
10000afac:     	orr	x8, x8, x25, lsl #32
10000afb0:     	extr	x25, x20, x25, #0x20
10000afb4:     	cmp	x22, x8
10000afb8:     	b.hs	0x10000aff8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1118>
10000afbc:     	lsl	x23, x8, #2
10000afc0:     	ldr	x20, [sp, #0x20]
10000afc4:     	ldr	x26, [sp, #0x8]
10000afc8:     	cbz	x22, 0x10000b79c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x18bc>
10000afcc:     	lsl	x24, x22, #2
10000afd0:     	mov	x0, x25
10000afd4:     	mov	x1, x23
10000afd8:     	mov	w2, #0x4                ; =4
10000afdc:     	mov	x3, x24
10000afe0:     	bl	0x1000631e0 <__RNvCsfLfy6EI15iL_7___rustc14___rust_realloc>
10000afe4:     	cbnz	x0, 0x10000b92c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a4c>
10000afe8:     	mov	w0, #0x4                ; =4
10000afec:     	mov	x1, x24
10000aff0:     	bl	0x10012bce8 <__RNvNtCs1OjIl8oxbrv_5alloc7raw_vec12handle_error>
10000aff4:     	b	0x10000b98c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1aac>
10000aff8:     	ldr	x20, [sp, #0x20]
10000affc:     	ldr	x26, [sp, #0x8]
10000b000:     	b	0x10000b948 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a68>
10000b004:     	ldur	x25, [x29, #-0xd0]
10000b008:     	stp	x27, x25, [x29, #-0xb0]
10000b00c:     	stur	xzr, [x29, #-0xa0]
10000b010:     	cbnz	x27, 0x10000a054 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x174>
10000b014:     	mov	x22, #0x0               ; =0
10000b018:     	ldp	x19, x25, [x29, #-0xb0]
10000b01c:     	ldr	x26, [sp, #0x8]
10000b020:     	cmp	x19, x22
10000b024:     	ldr	x20, [sp, #0x20]
10000b028:     	b.ls	0x10000b948 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a68>
10000b02c:     	lsl	x23, x19, #3
10000b030:     	cbz	x22, 0x10000b934 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a54>
10000b034:     	lsl	x24, x22, #3
10000b038:     	mov	x0, x25
10000b03c:     	mov	x1, x23
10000b040:     	mov	w2, #0x8                ; =8
10000b044:     	mov	x3, x24
10000b048:     	bl	0x1000631e0 <__RNvCsfLfy6EI15iL_7___rustc14___rust_realloc>
10000b04c:     	cbnz	x0, 0x10000b92c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a4c>
10000b050:     	mov	w0, #0x8                ; =8
10000b054:     	mov	x1, x24
10000b058:     	bl	0x10012bce8 <__RNvNtCs1OjIl8oxbrv_5alloc7raw_vec12handle_error>
10000b05c:     	b	0x10000b98c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1aac>
10000b060:     	ldr	x23, [sp, #0x110]
10000b064:     	ldrb	w24, [sp, #0x118]
10000b068:     	add	x9, sp, #0xf0
10000b06c:     	ldur	q0, [x9, #0x29]
10000b070:     	ldur	q1, [x9, #0x39]
10000b074:     	stp	q0, q1, [sp, #0x50]
10000b078:     	ldur	q0, [x9, #0x48]
10000b07c:     	stur	q0, [sp, #0x6f]
10000b080:     	extr	x25, x20, x8, #0x20
10000b084:     	lsr	x20, x20, #32
10000b088:     	mov	x26, x8
10000b08c:     	b	0x10000ac9c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xdbc>
10000b090:     	mov	x9, x8
10000b094:     	ldr	x20, [sp, #0x110]
10000b098:     	ldrb	w23, [sp, #0x118]
10000b09c:     	add	x8, sp, #0xf0
10000b0a0:     	ldur	q0, [x8, #0x29]
10000b0a4:     	ldur	q1, [x8, #0x39]
10000b0a8:     	stp	q0, q1, [sp, #0x50]
10000b0ac:     	ldur	q0, [x8, #0x48]
10000b0b0:     	stur	q0, [sp, #0x6f]
10000b0b4:     	mov	x24, x9
10000b0b8:     	b	0x10000af34 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1054>
10000b0bc:     	adrp	x0, 0x10012f000 <_aura_data_146+0x26a4>
10000b0c0:     	add	x0, x0, #0x230
10000b0c4:     	adrp	x2, 0x100160000 <_writev+0x100160000>
10000b0c8:     	add	x2, x2, #0x670
10000b0cc:     	mov	w1, #0x26               ; =38
10000b0d0:     	bl	0x10012bdd8 <__RNvNtCsl8K0bEFm1U0_4core6option13expect_failed>
10000b0d4:     	b	0x10000b98c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1aac>
10000b0d8:     	adrp	x0, 0x10012f000 <_aura_data_146+0x26a4>
10000b0dc:     	add	x0, x0, #0x230
10000b0e0:     	adrp	x2, 0x100160000 <_writev+0x100160000>
10000b0e4:     	add	x2, x2, #0x670
10000b0e8:     	mov	w1, #0x26               ; =38
10000b0ec:     	bl	0x10012bdd8 <__RNvNtCsl8K0bEFm1U0_4core6option13expect_failed>
10000b0f0:     	b	0x10000b98c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1aac>
10000b0f4:     	sub	x0, x29, #0xb0
10000b0f8:     	mov	x1, x22
10000b0fc:     	mov	x2, x25
10000b100:     	mov	w3, #0x4                ; =4
10000b104:     	mov	w4, #0x4                ; =4
10000b108:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000b10c:     	mov	x26, x21
10000b110:     	ldp	x24, x22, [x29, #-0xa8]
10000b114:     	ldur	s0, [x29, #-0xb8]
10000b118:     	cmp	x25, #0x10
10000b11c:     	b.lo	0x10000a2b8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x3d8>
10000b120:     	and	x8, x25, #0xfffffffffffffff0
10000b124:     	add	x9, x22, x8
10000b128:     	dup.4s	v1, v0[0]
10000b12c:     	add	x10, x27, #0x20
10000b130:     	add	x11, x24, x22, lsl #2
10000b134:     	add	x11, x11, #0x20
10000b138:     	and	x12, x25, #0xfffffffffffffff0
10000b13c:     	ldp	q2, q3, [x10, #-0x20]
10000b140:     	ldp	q4, q5, [x10], #0x40
10000b144:     	fadd.4s	v2, v1, v2
10000b148:     	fadd.4s	v3, v1, v3
10000b14c:     	fadd.4s	v4, v1, v4
10000b150:     	fadd.4s	v5, v1, v5
10000b154:     	stp	q2, q3, [x11, #-0x20]
10000b158:     	stp	q4, q5, [x11], #0x40
10000b15c:     	subs	x12, x12, #0x10
10000b160:     	b.ne	0x10000b13c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x125c>
10000b164:     	mov	x22, x9
10000b168:     	cmp	x25, x8
10000b16c:     	b.eq	0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000b170:     	sub	x10, x25, x8
10000b174:     	add	x8, x27, x8, lsl #2
10000b178:     	mov	x22, x9
10000b17c:     	ldr	s1, [x8], #0x4
10000b180:     	fadd	s1, s0, s1
10000b184:     	str	s1, [x24, x22, lsl #2]
10000b188:     	add	x22, x22, #0x1
10000b18c:     	subs	x10, x10, #0x1
10000b190:     	b.ne	0x10000b17c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x129c>
10000b194:     	b	0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000b198:     	sub	x0, x29, #0xb0
10000b19c:     	mov	x1, x22
10000b1a0:     	mov	x2, x25
10000b1a4:     	mov	w3, #0x8                ; =8
10000b1a8:     	mov	w4, #0x8                ; =8
10000b1ac:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000b1b0:     	mov	x26, x21
10000b1b4:     	ldp	x24, x22, [x29, #-0xa8]
10000b1b8:     	ldur	d0, [x29, #-0xb8]
10000b1bc:     	cmp	x25, #0x8
10000b1c0:     	b.lo	0x10000a374 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x494>
10000b1c4:     	and	x8, x25, #0xfffffffffffffff8
10000b1c8:     	add	x9, x22, x8
10000b1cc:     	dup.2d	v1, v0[0]
10000b1d0:     	add	x10, x27, #0x20
10000b1d4:     	add	x11, x24, x22, lsl #3
10000b1d8:     	add	x11, x11, #0x20
10000b1dc:     	and	x12, x25, #0xfffffffffffffff8
10000b1e0:     	ldp	q2, q3, [x10, #-0x20]
10000b1e4:     	ldp	q4, q5, [x10], #0x40
10000b1e8:     	fadd.2d	v2, v1, v2
10000b1ec:     	fadd.2d	v3, v1, v3
10000b1f0:     	fadd.2d	v4, v1, v4
10000b1f4:     	fadd.2d	v5, v1, v5
10000b1f8:     	stp	q2, q3, [x11, #-0x20]
10000b1fc:     	stp	q4, q5, [x11], #0x40
10000b200:     	subs	x12, x12, #0x8
10000b204:     	b.ne	0x10000b1e0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1300>
10000b208:     	mov	x22, x9
10000b20c:     	cmp	x25, x8
10000b210:     	b.eq	0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000b214:     	sub	x10, x25, x8
10000b218:     	add	x8, x27, x8, lsl #3
10000b21c:     	mov	x22, x9
10000b220:     	ldr	d1, [x8], #0x8
10000b224:     	fadd	d1, d0, d1
10000b228:     	str	d1, [x24, x22, lsl #3]
10000b22c:     	add	x22, x22, #0x1
10000b230:     	subs	x10, x10, #0x1
10000b234:     	b.ne	0x10000b220 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1340>
10000b238:     	b	0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000b23c:     	sub	x0, x29, #0xb0
10000b240:     	mov	x1, x22
10000b244:     	mov	x2, x25
10000b248:     	mov	w3, #0x4                ; =4
10000b24c:     	mov	w4, #0x4                ; =4
10000b250:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000b254:     	mov	x26, x21
10000b258:     	ldp	x24, x22, [x29, #-0xa8]
10000b25c:     	ldur	s0, [x29, #-0xb8]
10000b260:     	cmp	x25, #0x10
10000b264:     	b.lo	0x10000a4d0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x5f0>
10000b268:     	and	x8, x25, #0xfffffffffffffff0
10000b26c:     	add	x9, x22, x8
10000b270:     	dup.4s	v1, v0[0]
10000b274:     	add	x10, x27, #0x20
10000b278:     	add	x11, x24, x22, lsl #2
10000b27c:     	add	x11, x11, #0x20
10000b280:     	and	x12, x25, #0xfffffffffffffff0
10000b284:     	ldp	q2, q3, [x10, #-0x20]
10000b288:     	ldp	q4, q5, [x10], #0x40
10000b28c:     	fsub.4s	v2, v1, v2
10000b290:     	fsub.4s	v3, v1, v3
10000b294:     	fsub.4s	v4, v1, v4
10000b298:     	fsub.4s	v5, v1, v5
10000b29c:     	stp	q2, q3, [x11, #-0x20]
10000b2a0:     	stp	q4, q5, [x11], #0x40
10000b2a4:     	subs	x12, x12, #0x10
10000b2a8:     	b.ne	0x10000b284 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x13a4>
10000b2ac:     	mov	x22, x9
10000b2b0:     	cmp	x25, x8
10000b2b4:     	b.eq	0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000b2b8:     	sub	x10, x25, x8
10000b2bc:     	add	x8, x27, x8, lsl #2
10000b2c0:     	mov	x22, x9
10000b2c4:     	ldr	s1, [x8], #0x4
10000b2c8:     	fsub	s1, s0, s1
10000b2cc:     	str	s1, [x24, x22, lsl #2]
10000b2d0:     	add	x22, x22, #0x1
10000b2d4:     	subs	x10, x10, #0x1
10000b2d8:     	b.ne	0x10000b2c4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x13e4>
10000b2dc:     	b	0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000b2e0:     	sub	x0, x29, #0xb0
10000b2e4:     	mov	x1, x22
10000b2e8:     	mov	x2, x25
10000b2ec:     	mov	w3, #0x8                ; =8
10000b2f0:     	mov	w4, #0x8                ; =8
10000b2f4:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000b2f8:     	mov	x26, x21
10000b2fc:     	ldp	x24, x22, [x29, #-0xa8]
10000b300:     	ldur	d0, [x29, #-0xb8]
10000b304:     	cmp	x25, #0x8
10000b308:     	b.lo	0x10000a4f8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x618>
10000b30c:     	and	x8, x25, #0xfffffffffffffff8
10000b310:     	add	x9, x22, x8
10000b314:     	dup.2d	v1, v0[0]
10000b318:     	add	x10, x27, #0x20
10000b31c:     	add	x11, x24, x22, lsl #3
10000b320:     	add	x11, x11, #0x20
10000b324:     	and	x12, x25, #0xfffffffffffffff8
10000b328:     	ldp	q2, q3, [x10, #-0x20]
10000b32c:     	ldp	q4, q5, [x10], #0x40
10000b330:     	fsub.2d	v2, v1, v2
10000b334:     	fsub.2d	v3, v1, v3
10000b338:     	fsub.2d	v4, v1, v4
10000b33c:     	fsub.2d	v5, v1, v5
10000b340:     	stp	q2, q3, [x11, #-0x20]
10000b344:     	stp	q4, q5, [x11], #0x40
10000b348:     	subs	x12, x12, #0x8
10000b34c:     	b.ne	0x10000b328 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1448>
10000b350:     	mov	x22, x9
10000b354:     	cmp	x25, x8
10000b358:     	b.eq	0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000b35c:     	sub	x10, x25, x8
10000b360:     	add	x8, x27, x8, lsl #3
10000b364:     	mov	x22, x9
10000b368:     	ldr	d1, [x8], #0x8
10000b36c:     	fsub	d1, d0, d1
10000b370:     	str	d1, [x24, x22, lsl #3]
10000b374:     	add	x22, x22, #0x1
10000b378:     	subs	x10, x10, #0x1
10000b37c:     	b.ne	0x10000b368 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1488>
10000b380:     	b	0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000b384:     	sub	x0, x29, #0xb0
10000b388:     	mov	x1, x22
10000b38c:     	mov	x2, x25
10000b390:     	mov	w3, #0x4                ; =4
10000b394:     	mov	w4, #0x4                ; =4
10000b398:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000b39c:     	mov	x26, x21
10000b3a0:     	ldp	x24, x22, [x29, #-0xa8]
10000b3a4:     	ldur	s0, [x29, #-0xb8]
10000b3a8:     	cmp	x25, #0x10
10000b3ac:     	b.lo	0x10000a51c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x63c>
10000b3b0:     	and	x8, x25, #0xfffffffffffffff0
10000b3b4:     	add	x9, x22, x8
10000b3b8:     	dup.4s	v1, v0[0]
10000b3bc:     	add	x10, x27, #0x20
10000b3c0:     	add	x11, x24, x22, lsl #2
10000b3c4:     	add	x11, x11, #0x20
10000b3c8:     	and	x12, x25, #0xfffffffffffffff0
10000b3cc:     	ldp	q2, q3, [x10, #-0x20]
10000b3d0:     	ldp	q4, q5, [x10], #0x40
10000b3d4:     	fadd.4s	v2, v1, v2
10000b3d8:     	fadd.4s	v3, v1, v3
10000b3dc:     	fadd.4s	v4, v1, v4
10000b3e0:     	fadd.4s	v5, v1, v5
10000b3e4:     	stp	q2, q3, [x11, #-0x20]
10000b3e8:     	stp	q4, q5, [x11], #0x40
10000b3ec:     	subs	x12, x12, #0x10
10000b3f0:     	b.ne	0x10000b3cc <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x14ec>
10000b3f4:     	mov	x22, x9
10000b3f8:     	cmp	x25, x8
10000b3fc:     	b.eq	0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000b400:     	sub	x10, x25, x8
10000b404:     	add	x8, x27, x8, lsl #2
10000b408:     	mov	x22, x9
10000b40c:     	ldr	s1, [x8], #0x4
10000b410:     	fadd	s1, s0, s1
10000b414:     	str	s1, [x24, x22, lsl #2]
10000b418:     	add	x22, x22, #0x1
10000b41c:     	subs	x10, x10, #0x1
10000b420:     	b.ne	0x10000b40c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x152c>
10000b424:     	b	0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000b428:     	sub	x0, x29, #0xb0
10000b42c:     	mov	x1, x22
10000b430:     	mov	x2, x25
10000b434:     	mov	w3, #0x8                ; =8
10000b438:     	mov	w4, #0x8                ; =8
10000b43c:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000b440:     	mov	x26, x21
10000b444:     	ldp	x24, x22, [x29, #-0xa8]
10000b448:     	ldur	d0, [x29, #-0xb8]
10000b44c:     	cmp	x25, #0x8
10000b450:     	b.lo	0x10000a540 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x660>
10000b454:     	and	x8, x25, #0xfffffffffffffff8
10000b458:     	add	x9, x22, x8
10000b45c:     	dup.2d	v1, v0[0]
10000b460:     	add	x10, x27, #0x20
10000b464:     	add	x11, x24, x22, lsl #3
10000b468:     	add	x11, x11, #0x20
10000b46c:     	and	x12, x25, #0xfffffffffffffff8
10000b470:     	ldp	q2, q3, [x10, #-0x20]
10000b474:     	ldp	q4, q5, [x10], #0x40
10000b478:     	fadd.2d	v2, v1, v2
10000b47c:     	fadd.2d	v3, v1, v3
10000b480:     	fadd.2d	v4, v1, v4
10000b484:     	fadd.2d	v5, v1, v5
10000b488:     	stp	q2, q3, [x11, #-0x20]
10000b48c:     	stp	q4, q5, [x11], #0x40
10000b490:     	subs	x12, x12, #0x8
10000b494:     	b.ne	0x10000b470 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1590>
10000b498:     	mov	x22, x9
10000b49c:     	cmp	x25, x8
10000b4a0:     	b.eq	0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000b4a4:     	sub	x10, x25, x8
10000b4a8:     	add	x8, x27, x8, lsl #3
10000b4ac:     	mov	x22, x9
10000b4b0:     	ldr	d1, [x8], #0x8
10000b4b4:     	fadd	d1, d0, d1
10000b4b8:     	str	d1, [x24, x22, lsl #3]
10000b4bc:     	add	x22, x22, #0x1
10000b4c0:     	subs	x10, x10, #0x1
10000b4c4:     	b.ne	0x10000b4b0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x15d0>
10000b4c8:     	b	0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000b4cc:     	sub	x0, x29, #0xb0
10000b4d0:     	mov	x1, x22
10000b4d4:     	mov	x2, x25
10000b4d8:     	mov	w3, #0x4                ; =4
10000b4dc:     	mov	w4, #0x4                ; =4
10000b4e0:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000b4e4:     	mov	x26, x21
10000b4e8:     	ldp	x24, x22, [x29, #-0xa8]
10000b4ec:     	ldur	s0, [x29, #-0xb8]
10000b4f0:     	cmp	x25, #0x10
10000b4f4:     	b.lo	0x10000a564 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x684>
10000b4f8:     	and	x8, x25, #0xfffffffffffffff0
10000b4fc:     	add	x9, x22, x8
10000b500:     	dup.4s	v1, v0[0]
10000b504:     	add	x10, x27, #0x20
10000b508:     	add	x11, x24, x22, lsl #2
10000b50c:     	add	x11, x11, #0x20
10000b510:     	and	x12, x25, #0xfffffffffffffff0
10000b514:     	ldp	q2, q3, [x10, #-0x20]
10000b518:     	ldp	q4, q5, [x10], #0x40
10000b51c:     	fsub.4s	v2, v2, v1
10000b520:     	fsub.4s	v3, v3, v1
10000b524:     	fsub.4s	v4, v4, v1
10000b528:     	fsub.4s	v5, v5, v1
10000b52c:     	stp	q2, q3, [x11, #-0x20]
10000b530:     	stp	q4, q5, [x11], #0x40
10000b534:     	subs	x12, x12, #0x10
10000b538:     	b.ne	0x10000b514 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1634>
10000b53c:     	mov	x22, x9
10000b540:     	cmp	x25, x8
10000b544:     	b.eq	0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000b548:     	sub	x10, x25, x8
10000b54c:     	add	x8, x27, x8, lsl #2
10000b550:     	mov	x22, x9
10000b554:     	ldr	s1, [x8], #0x4
10000b558:     	fsub	s1, s1, s0
10000b55c:     	str	s1, [x24, x22, lsl #2]
10000b560:     	add	x22, x22, #0x1
10000b564:     	subs	x10, x10, #0x1
10000b568:     	b.ne	0x10000b554 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1674>
10000b56c:     	b	0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000b570:     	sub	x0, x29, #0xb0
10000b574:     	mov	x1, x22
10000b578:     	mov	x2, x25
10000b57c:     	mov	w3, #0x8                ; =8
10000b580:     	mov	w4, #0x8                ; =8
10000b584:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000b588:     	mov	x26, x21
10000b58c:     	ldp	x24, x22, [x29, #-0xa8]
10000b590:     	ldur	d0, [x29, #-0xb8]
10000b594:     	cmp	x25, #0x8
10000b598:     	b.lo	0x10000a588 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x6a8>
10000b59c:     	and	x8, x25, #0xfffffffffffffff8
10000b5a0:     	add	x9, x22, x8
10000b5a4:     	dup.2d	v1, v0[0]
10000b5a8:     	add	x10, x27, #0x20
10000b5ac:     	add	x11, x24, x22, lsl #3
10000b5b0:     	add	x11, x11, #0x20
10000b5b4:     	and	x12, x25, #0xfffffffffffffff8
10000b5b8:     	ldp	q2, q3, [x10, #-0x20]
10000b5bc:     	ldp	q4, q5, [x10], #0x40
10000b5c0:     	fsub.2d	v2, v2, v1
10000b5c4:     	fsub.2d	v3, v3, v1
10000b5c8:     	fsub.2d	v4, v4, v1
10000b5cc:     	fsub.2d	v5, v5, v1
10000b5d0:     	stp	q2, q3, [x11, #-0x20]
10000b5d4:     	stp	q4, q5, [x11], #0x40
10000b5d8:     	subs	x12, x12, #0x8
10000b5dc:     	b.ne	0x10000b5b8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x16d8>
10000b5e0:     	mov	x22, x9
10000b5e4:     	cmp	x25, x8
10000b5e8:     	b.eq	0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000b5ec:     	sub	x10, x25, x8
10000b5f0:     	add	x8, x27, x8, lsl #3
10000b5f4:     	mov	x22, x9
10000b5f8:     	ldr	d1, [x8], #0x8
10000b5fc:     	fsub	d1, d1, d0
10000b600:     	str	d1, [x24, x22, lsl #3]
10000b604:     	add	x22, x22, #0x1
10000b608:     	subs	x10, x10, #0x1
10000b60c:     	b.ne	0x10000b5f8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1718>
10000b610:     	b	0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000b614:     	sub	x0, x29, #0xb0
10000b618:     	mov	x1, x22
10000b61c:     	mov	x2, x25
10000b620:     	mov	w3, #0x4                ; =4
10000b624:     	mov	w4, #0x4                ; =4
10000b628:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000b62c:     	mov	x26, x21
10000b630:     	ldp	x24, x22, [x29, #-0xa8]
10000b634:     	ldur	s0, [x29, #-0xb8]
10000b638:     	cmp	x25, #0x10
10000b63c:     	b.lo	0x10000a668 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x788>
10000b640:     	and	x8, x25, #0xfffffffffffffff0
10000b644:     	add	x9, x22, x8
10000b648:     	add	x10, x27, #0x20
10000b64c:     	add	x11, x24, x22, lsl #2
10000b650:     	add	x11, x11, #0x20
10000b654:     	and	x12, x25, #0xfffffffffffffff0
10000b658:     	ldp	q1, q2, [x10, #-0x20]
10000b65c:     	ldp	q3, q4, [x10], #0x40
10000b660:     	fmul.4s	v1, v1, v0[0]
10000b664:     	fmul.4s	v2, v2, v0[0]
10000b668:     	fmul.4s	v3, v3, v0[0]
10000b66c:     	fmul.4s	v4, v4, v0[0]
10000b670:     	stp	q1, q2, [x11, #-0x20]
10000b674:     	stp	q3, q4, [x11], #0x40
10000b678:     	subs	x12, x12, #0x10
10000b67c:     	b.ne	0x10000b658 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1778>
10000b680:     	mov	x22, x9
10000b684:     	cmp	x25, x8
10000b688:     	b.eq	0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000b68c:     	sub	x10, x25, x8
10000b690:     	add	x8, x27, x8, lsl #2
10000b694:     	mov	x22, x9
10000b698:     	ldr	s1, [x8], #0x4
10000b69c:     	fmul	s1, s0, s1
10000b6a0:     	str	s1, [x24, x22, lsl #2]
10000b6a4:     	add	x22, x22, #0x1
10000b6a8:     	subs	x10, x10, #0x1
10000b6ac:     	b.ne	0x10000b698 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x17b8>
10000b6b0:     	b	0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000b6b4:     	sub	x0, x29, #0xb0
10000b6b8:     	mov	x1, x22
10000b6bc:     	mov	x2, x25
10000b6c0:     	mov	w3, #0x4                ; =4
10000b6c4:     	mov	w4, #0x4                ; =4
10000b6c8:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000b6cc:     	mov	x26, x21
10000b6d0:     	ldp	x24, x22, [x29, #-0xa8]
10000b6d4:     	ldur	s0, [x29, #-0xb8]
10000b6d8:     	cmp	x25, #0x10
10000b6dc:     	b.lo	0x10000a6dc <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x7fc>
10000b6e0:     	and	x8, x25, #0xfffffffffffffff0
10000b6e4:     	add	x9, x22, x8
10000b6e8:     	add	x10, x27, #0x20
10000b6ec:     	add	x11, x24, x22, lsl #2
10000b6f0:     	add	x11, x11, #0x20
10000b6f4:     	and	x12, x25, #0xfffffffffffffff0
10000b6f8:     	ldp	q1, q2, [x10, #-0x20]
10000b6fc:     	ldp	q3, q4, [x10], #0x40
10000b700:     	fmul.4s	v1, v1, v0[0]
10000b704:     	fmul.4s	v2, v2, v0[0]
10000b708:     	fmul.4s	v3, v3, v0[0]
10000b70c:     	fmul.4s	v4, v4, v0[0]
10000b710:     	stp	q1, q2, [x11, #-0x20]
10000b714:     	stp	q3, q4, [x11], #0x40
10000b718:     	subs	x12, x12, #0x10
10000b71c:     	b.ne	0x10000b6f8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1818>
10000b720:     	mov	x22, x9
10000b724:     	cmp	x25, x8
10000b728:     	b.eq	0x10000b750 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1870>
10000b72c:     	sub	x10, x25, x8
10000b730:     	add	x8, x27, x8, lsl #2
10000b734:     	mov	x22, x9
10000b738:     	ldr	s1, [x8], #0x4
10000b73c:     	fmul	s1, s0, s1
10000b740:     	str	s1, [x24, x22, lsl #2]
10000b744:     	add	x22, x22, #0x1
10000b748:     	subs	x10, x10, #0x1
10000b74c:     	b.ne	0x10000b738 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1858>
10000b750:     	stur	x22, [x29, #-0xa0]
10000b754:     	ldp	x21, x25, [x29, #-0xb0]
10000b758:     	ldur	x22, [x29, #-0xa0]
10000b75c:     	cmp	x21, x22
10000b760:     	ldr	x20, [sp, #0x20]
10000b764:     	b.ls	0x10000b948 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a68>
10000b768:     	lsl	x23, x21, #2
10000b76c:     	cbz	x22, 0x10000b79c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x18bc>
10000b770:     	lsl	x24, x22, #2
10000b774:     	mov	x0, x25
10000b778:     	mov	x1, x23
10000b77c:     	mov	w2, #0x4                ; =4
10000b780:     	mov	x3, x24
10000b784:     	bl	0x1000631e0 <__RNvCsfLfy6EI15iL_7___rustc14___rust_realloc>
10000b788:     	cbnz	x0, 0x10000b92c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a4c>
10000b78c:     	mov	w0, #0x4                ; =4
10000b790:     	mov	x1, x24
10000b794:     	bl	0x10012bce8 <__RNvNtCs1OjIl8oxbrv_5alloc7raw_vec12handle_error>
10000b798:     	b	0x10000b98c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1aac>
10000b79c:     	mov	x0, x25
10000b7a0:     	mov	x1, x23
10000b7a4:     	mov	w2, #0x4                ; =4
10000b7a8:     	bl	0x1000631dc <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10000b7ac:     	mov	w25, #0x4               ; =4
10000b7b0:     	b	0x10000b948 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a68>
10000b7b4:     	sub	x0, x29, #0xb0
10000b7b8:     	mov	x1, x22
10000b7bc:     	mov	x2, x25
10000b7c0:     	mov	w3, #0x8                ; =8
10000b7c4:     	mov	w4, #0x8                ; =8
10000b7c8:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000b7cc:     	mov	x26, x21
10000b7d0:     	ldp	x24, x22, [x29, #-0xa8]
10000b7d4:     	ldur	d0, [x29, #-0xb8]
10000b7d8:     	cmp	x25, #0x8
10000b7dc:     	b.lo	0x10000a8ac <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x9cc>
10000b7e0:     	and	x8, x25, #0xfffffffffffffff8
10000b7e4:     	add	x9, x22, x8
10000b7e8:     	add	x10, x27, #0x20
10000b7ec:     	add	x11, x24, x22, lsl #3
10000b7f0:     	add	x11, x11, #0x20
10000b7f4:     	and	x12, x25, #0xfffffffffffffff8
10000b7f8:     	ldp	q1, q2, [x10, #-0x20]
10000b7fc:     	ldp	q3, q4, [x10], #0x40
10000b800:     	fmul.2d	v1, v1, v0[0]
10000b804:     	fmul.2d	v2, v2, v0[0]
10000b808:     	fmul.2d	v3, v3, v0[0]
10000b80c:     	fmul.2d	v4, v4, v0[0]
10000b810:     	stp	q1, q2, [x11, #-0x20]
10000b814:     	stp	q3, q4, [x11], #0x40
10000b818:     	subs	x12, x12, #0x8
10000b81c:     	b.ne	0x10000b7f8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1918>
10000b820:     	mov	x22, x9
10000b824:     	cmp	x25, x8
10000b828:     	b.eq	0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000b82c:     	sub	x10, x25, x8
10000b830:     	add	x8, x27, x8, lsl #3
10000b834:     	mov	x22, x9
10000b838:     	ldr	d1, [x8], #0x8
10000b83c:     	fmul	d1, d0, d1
10000b840:     	str	d1, [x24, x22, lsl #3]
10000b844:     	add	x22, x22, #0x1
10000b848:     	subs	x10, x10, #0x1
10000b84c:     	b.ne	0x10000b838 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1958>
10000b850:     	b	0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000b854:     	sub	x0, x29, #0xb0
10000b858:     	mov	x1, x22
10000b85c:     	mov	x2, x25
10000b860:     	mov	w3, #0x8                ; =8
10000b864:     	mov	w4, #0x8                ; =8
10000b868:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000b86c:     	mov	x26, x21
10000b870:     	ldp	x24, x22, [x29, #-0xa8]
10000b874:     	ldur	d0, [x29, #-0xb8]
10000b878:     	cmp	x25, #0x8
10000b87c:     	b.lo	0x10000a920 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0xa40>
10000b880:     	and	x8, x25, #0xfffffffffffffff8
10000b884:     	add	x9, x22, x8
10000b888:     	add	x10, x27, #0x20
10000b88c:     	add	x11, x24, x22, lsl #3
10000b890:     	add	x11, x11, #0x20
10000b894:     	and	x12, x25, #0xfffffffffffffff8
10000b898:     	ldp	q1, q2, [x10, #-0x20]
10000b89c:     	ldp	q3, q4, [x10], #0x40
10000b8a0:     	fmul.2d	v1, v1, v0[0]
10000b8a4:     	fmul.2d	v2, v2, v0[0]
10000b8a8:     	fmul.2d	v3, v3, v0[0]
10000b8ac:     	fmul.2d	v4, v4, v0[0]
10000b8b0:     	stp	q1, q2, [x11, #-0x20]
10000b8b4:     	stp	q3, q4, [x11], #0x40
10000b8b8:     	subs	x12, x12, #0x8
10000b8bc:     	b.ne	0x10000b898 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x19b8>
10000b8c0:     	mov	x22, x9
10000b8c4:     	cmp	x25, x8
10000b8c8:     	b.eq	0x10000b8f0 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a10>
10000b8cc:     	sub	x10, x25, x8
10000b8d0:     	add	x8, x27, x8, lsl #3
10000b8d4:     	mov	x22, x9
10000b8d8:     	ldr	d1, [x8], #0x8
10000b8dc:     	fmul	d1, d0, d1
10000b8e0:     	str	d1, [x24, x22, lsl #3]
10000b8e4:     	add	x22, x22, #0x1
10000b8e8:     	subs	x10, x10, #0x1
10000b8ec:     	b.ne	0x10000b8d8 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x19f8>
10000b8f0:     	stur	x22, [x29, #-0xa0]
10000b8f4:     	ldp	x21, x25, [x29, #-0xb0]
10000b8f8:     	ldur	x22, [x29, #-0xa0]
10000b8fc:     	cmp	x21, x22
10000b900:     	ldr	x20, [sp, #0x20]
10000b904:     	b.ls	0x10000b948 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a68>
10000b908:     	lsl	x23, x21, #3
10000b90c:     	cbz	x22, 0x10000b934 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a54>
10000b910:     	lsl	x24, x22, #3
10000b914:     	mov	x0, x25
10000b918:     	mov	x1, x23
10000b91c:     	mov	w2, #0x8                ; =8
10000b920:     	mov	x3, x24
10000b924:     	bl	0x1000631e0 <__RNvCsfLfy6EI15iL_7___rustc14___rust_realloc>
10000b928:     	cbz	x0, 0x10000b980 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1aa0>
10000b92c:     	mov	x25, x0
10000b930:     	b	0x10000b948 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1a68>
10000b934:     	mov	x0, x25
10000b938:     	mov	x1, x23
10000b93c:     	mov	w2, #0x8                ; =8
10000b940:     	bl	0x1000631dc <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10000b944:     	mov	w25, #0x8               ; =8
10000b948:     	stp	x25, x22, [sp, #0x40]
10000b94c:     	str	x26, [sp, #0x38]
10000b950:     	add	x2, sp, #0x38
10000b954:     	ldp	x1, x8, [sp, #0x28]
10000b958:     	mov	x0, x20
10000b95c:     	bl	0x10000bb58 <__ZN13aura_compiler13runtime_value10ArrayValue3new17he7709f9a4e9fd09eE>
10000b960:     	add	sp, sp, #0x1e0
10000b964:     	ldp	x29, x30, [sp, #0x50]
10000b968:     	ldp	x20, x19, [sp, #0x40]
10000b96c:     	ldp	x22, x21, [sp, #0x30]
10000b970:     	ldp	x24, x23, [sp, #0x20]
10000b974:     	ldp	x26, x25, [sp, #0x10]
10000b978:     	ldp	x28, x27, [sp], #0x60
10000b97c:     	ret
10000b980:     	mov	w0, #0x8                ; =8
10000b984:     	mov	x1, x24
10000b988:     	bl	0x10012bce8 <__RNvNtCs1OjIl8oxbrv_5alloc7raw_vec12handle_error>
10000b98c:     	brk	#0x1
10000b990:     	b	0x10000b998 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1ab8>
10000b994:     	b	0x10000b9ac <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1acc>
10000b998:     	mov	x21, x0
10000b99c:     	mov	x0, x25
10000b9a0:     	mov	x1, x23
10000b9a4:     	mov	w2, #0x8                ; =8
10000b9a8:     	b	0x10000ba28 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b48>
10000b9ac:     	mov	x21, x0
10000b9b0:     	mov	x0, x25
10000b9b4:     	mov	x1, x23
10000b9b8:     	b	0x10000ba24 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b44>
10000b9bc:     	mov	x21, x0
10000b9c0:     	ldur	x8, [x29, #-0xb0]
10000b9c4:     	cbnz	x8, 0x10000ba00 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b20>
10000b9c8:     	b	0x10000ba2c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b4c>
10000b9cc:     	mov	x21, x0
10000b9d0:     	ldur	x8, [x29, #-0xb0]
10000b9d4:     	cbnz	x8, 0x10000ba1c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b3c>
10000b9d8:     	b	0x10000ba2c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b4c>
10000b9dc:     	b	0x10000b9f4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b14>
10000b9e0:     	b	0x10000ba10 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b30>
10000b9e4:     	b	0x10000b9f4 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b14>
10000b9e8:     	b	0x10000ba10 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b30>
10000b9ec:     	mov	x21, x0
10000b9f0:     	b	0x10000ba30 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b50>
10000b9f4:     	mov	x21, x0
10000b9f8:     	ldur	x8, [x29, #-0xb0]
10000b9fc:     	cbz	x8, 0x10000ba2c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b4c>
10000ba00:     	ldur	x0, [x29, #-0xa8]
10000ba04:     	lsl	x1, x8, #3
10000ba08:     	mov	w2, #0x8                ; =8
10000ba0c:     	b	0x10000ba28 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b48>
10000ba10:     	mov	x21, x0
10000ba14:     	ldur	x8, [x29, #-0xb0]
10000ba18:     	cbz	x8, 0x10000ba2c <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b4c>
10000ba1c:     	ldur	x0, [x29, #-0xa8]
10000ba20:     	lsl	x1, x8, #2
10000ba24:     	mov	w2, #0x4                ; =4
10000ba28:     	bl	0x1000631dc <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10000ba2c:     	ldr	x20, [sp, #0x28]
10000ba30:     	cbz	x20, 0x10000ba44 <__ZN13aura_compiler13runtime_value10ArrayValue13scalar_binary17h5d7def3f45d613cfE+0x1b64>
10000ba34:     	lsl	x1, x20, #3
10000ba38:     	ldr	x0, [sp, #0x20]
10000ba3c:     	mov	w2, #0x8                ; =8
10000ba40:     	bl	0x1000631dc <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10000ba44:     	mov	x0, x21
10000ba48:     	bl	0x10012c1c0 <_writev+0x10012c1c0>

000000010000e924 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE>:
10000e924:     	sub	sp, sp, #0x130
10000e928:     	stp	x28, x27, [sp, #0xe0]
10000e92c:     	stp	x24, x23, [sp, #0xf0]
10000e930:     	stp	x22, x21, [sp, #0x100]
10000e934:     	stp	x20, x19, [sp, #0x110]
10000e938:     	stp	x29, x30, [sp, #0x120]
10000e93c:     	add	x29, sp, #0x120
10000e940:     	mov	x24, x5
10000e944:     	mov	x22, x4
10000e948:     	mov	x20, x3
10000e94c:     	mov	x23, x2
10000e950:     	mov	x21, x1
10000e954:     	mov	x19, x0
10000e958:     	cbz	x6, 0x10000e984 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x60>
10000e95c:     	adrp	x1, 0x10012e000 <_aura_data_146+0x16a4>
10000e960:     	add	x1, x1, #0xf2c
10000e964:     	adrp	x2, 0x10012f000 <_aura_data_146+0x26a4>
10000e968:     	add	x2, x2, #0x406
10000e96c:     	add	x0, sp, #0x38
10000e970:     	mov	w3, #0x43               ; =67
10000e974:     	bl	0x10002ef38 <__ZN13aura_compiler4diag10Diagnostic5coded17h0c8f829f44e62944E>
10000e978:     	ldr	x8, [sp, #0x38]
10000e97c:     	cmp	x8, #0x2
10000e980:     	b.ne	0x10000ea28 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x104>
10000e984:     	cbnz	x23, 0x10000eba0 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x27c>
10000e988:     	mov	w8, #0x4                ; =4
10000e98c:     	stp	x23, x8, [sp, #0x40]
10000e990:     	str	xzr, [sp, #0x50]
10000e994:     	ldr	x8, [sp, #0x50]
10000e998:     	ldur	q0, [sp, #0x40]
10000e99c:     	str	q0, [sp]
10000e9a0:     	str	x8, [sp, #0x10]
10000e9a4:     	cmp	x24, #0x1
10000e9a8:     	b.gt	0x10000e9e8 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0xc4>
10000e9ac:     	cmp	x22, x23
10000e9b0:     	csel	x22, x22, x23, lo
10000e9b4:     	cbnz	x24, 0x10000ea54 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x130>
10000e9b8:     	ldr	x1, [sp, #0x10]
10000e9bc:     	ldr	x8, [sp]
10000e9c0:     	sub	x8, x8, x1
10000e9c4:     	cmp	x22, x8
10000e9c8:     	b.hi	0x10000ec6c <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x348>
10000e9cc:     	cbz	x22, 0x10000eae0 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x1bc>
10000e9d0:     	ldr	x8, [sp, #0x8]
10000e9d4:     	cmp	x22, #0x10
10000e9d8:     	b.hs	0x10000ec8c <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x368>
10000e9dc:     	mov	x10, #0x0               ; =0
10000e9e0:     	mov	x9, x1
10000e9e4:     	b	0x10000ece0 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x3bc>
10000e9e8:     	cmp	x24, #0x2
10000e9ec:     	b.ne	0x10000ea84 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x160>
10000e9f0:     	cmp	x22, x23
10000e9f4:     	csel	x22, x22, x23, lo
10000e9f8:     	ldr	x1, [sp, #0x10]
10000e9fc:     	ldr	x8, [sp]
10000ea00:     	sub	x8, x8, x1
10000ea04:     	cmp	x22, x8
10000ea08:     	b.hi	0x10000ed10 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x3ec>
10000ea0c:     	cbz	x22, 0x10000eae0 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x1bc>
10000ea10:     	ldr	x8, [sp, #0x8]
10000ea14:     	cmp	x22, #0x10
10000ea18:     	b.hs	0x10000ed30 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x40c>
10000ea1c:     	mov	x10, #0x0               ; =0
10000ea20:     	mov	x9, x1
10000ea24:     	b	0x10000ed84 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x460>
10000ea28:     	ldur	q0, [sp, #0x58]
10000ea2c:     	ldur	q1, [sp, #0x68]
10000ea30:     	stp	q0, q1, [x19, #0x20]
10000ea34:     	ldur	q0, [sp, #0x78]
10000ea38:     	str	q0, [x19, #0x40]
10000ea3c:     	ldr	x8, [sp, #0x88]
10000ea40:     	str	x8, [x19, #0x50]
10000ea44:     	ldur	q0, [sp, #0x38]
10000ea48:     	ldur	q1, [sp, #0x48]
10000ea4c:     	stp	q0, q1, [x19]
10000ea50:     	b	0x10000ef10 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x5ec>
10000ea54:     	ldr	x1, [sp, #0x10]
10000ea58:     	ldr	x8, [sp]
10000ea5c:     	sub	x8, x8, x1
10000ea60:     	cmp	x22, x8
10000ea64:     	b.hi	0x10000edb4 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x490>
10000ea68:     	cbz	x22, 0x10000eae0 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x1bc>
10000ea6c:     	ldr	x8, [sp, #0x8]
10000ea70:     	cmp	x22, #0x10
10000ea74:     	b.hs	0x10000edd4 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x4b0>
10000ea78:     	mov	x10, #0x0               ; =0
10000ea7c:     	mov	x9, x1
10000ea80:     	b	0x10000ee28 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x504>
10000ea84:     	cbz	x22, 0x10000eaa8 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x184>
10000ea88:     	mov	x8, #0x0                ; =0
10000ea8c:     	lsl	x9, x22, #2
10000ea90:     	ldr	s0, [x20, x8, lsl #2]
10000ea94:     	fcmp	s0, #0.0
10000ea98:     	b.eq	0x10000eae8 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x1c4>
10000ea9c:     	add	x8, x8, #0x1
10000eaa0:     	subs	x9, x9, #0x4
10000eaa4:     	b.ne	0x10000ea90 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x16c>
10000eaa8:     	cmp	x22, x23
10000eaac:     	csel	x22, x22, x23, lo
10000eab0:     	ldr	x1, [sp, #0x10]
10000eab4:     	ldr	x8, [sp]
10000eab8:     	sub	x8, x8, x1
10000eabc:     	cmp	x22, x8
10000eac0:     	b.hi	0x10000ee58 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x534>
10000eac4:     	cbz	x22, 0x10000eae0 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x1bc>
10000eac8:     	ldr	x8, [sp, #0x8]
10000eacc:     	cmp	x22, #0x10
10000ead0:     	b.hs	0x10000ee78 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x554>
10000ead4:     	mov	x10, #0x0               ; =0
10000ead8:     	mov	x9, x1
10000eadc:     	b	0x10000eecc <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x5a8>
10000eae0:     	mov	x9, x1
10000eae4:     	b	0x10000eef8 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x5d4>
10000eae8:     	str	x8, [sp, #0x90]
10000eaec:     	adrp	x8, 0x10012f000 <_aura_data_146+0x26a4>
10000eaf0:     	add	x8, x8, #0x21e
10000eaf4:     	mov	w9, #0x12               ; =18
10000eaf8:     	stp	x8, x9, [sp, #0x20]
10000eafc:     	adrp	x8, 0x100130000 <_aura_data_146+0x36a4>
10000eb00:     	add	x8, x8, #0x998
10000eb04:     	mov	w9, #0x8                ; =8
10000eb08:     	stp	x8, x9, [x29, #-0x78]
10000eb0c:     	sub	x8, x29, #0x78
10000eb10:     	adrp	x9, 0x100038000 <__ZN3std6thread5local17LocalKey$LT$T$GT$8try_with17hafda283f2a50e3b8E+0x58>
10000eb14:     	add	x9, x9, #0x688
10000eb18:     	stp	x8, x9, [sp, #0x38]
10000eb1c:     	add	x8, sp, #0x20
10000eb20:     	stp	x8, x9, [sp, #0x48]
10000eb24:     	add	x8, sp, #0x90
10000eb28:     	adrp	x9, 0x100122000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
10000eb2c:     	add	x9, x9, #0xc8c
10000eb30:     	stp	x8, x9, [sp, #0x58]
10000eb34:     	adrp	x0, 0x100141000 <_writev+0x100141000>
10000eb38:     	add	x0, x0, #0x103
10000eb3c:     	sub	x8, x29, #0x60
10000eb40:     	add	x1, sp, #0x38
10000eb44:     	bl	0x10011927c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
10000eb48:     	adrp	x1, 0x10012f000 <_aura_data_146+0x26a4>
10000eb4c:     	add	x1, x1, #0x218
10000eb50:     	add	x0, sp, #0x38
10000eb54:     	sub	x2, x29, #0x60
10000eb58:     	bl	0x10002f00c <__ZN13aura_compiler4diag10Diagnostic5coded17hc0c6d1b6375dce8eE>
10000eb5c:     	ldur	q0, [sp, #0x58]
10000eb60:     	ldur	q1, [sp, #0x68]
10000eb64:     	stp	q0, q1, [x19, #0x20]
10000eb68:     	ldur	q0, [sp, #0x78]
10000eb6c:     	str	q0, [x19, #0x40]
10000eb70:     	ldr	x8, [sp, #0x88]
10000eb74:     	str	x8, [x19, #0x50]
10000eb78:     	ldur	q0, [sp, #0x38]
10000eb7c:     	ldur	q1, [sp, #0x48]
10000eb80:     	stp	q0, q1, [x19]
10000eb84:     	ldr	x8, [sp]
10000eb88:     	cbz	x8, 0x10000ef10 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x5ec>
10000eb8c:     	ldr	x0, [sp, #0x8]
10000eb90:     	lsl	x1, x8, #2
10000eb94:     	mov	w2, #0x4                ; =4
10000eb98:     	bl	0x1000631dc <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10000eb9c:     	b	0x10000ef10 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x5ec>
10000eba0:     	sub	x0, x29, #0x60
10000eba4:     	mov	x1, #0x0                ; =0
10000eba8:     	mov	w2, #0x4                ; =4
10000ebac:     	mov	x3, x23
10000ebb0:     	mov	w4, #0x4                ; =4
10000ebb4:     	mov	w5, #0x4                ; =4
10000ebb8:     	bl	0x100125708 <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$11finish_grow17h8de8c598a5dc063cE>
10000ebbc:     	ldur	w8, [x29, #-0x60]
10000ebc0:     	tbz	w8, #0x0, 0x10000ec64 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x340>
10000ebc4:     	adrp	x8, 0x10012f000 <_aura_data_146+0x26a4>
10000ebc8:     	add	x8, x8, #0x201
10000ebcc:     	mov	w9, #0x17               ; =23
10000ebd0:     	stp	x8, x9, [sp, #0x90]
10000ebd4:     	stur	x23, [x29, #-0x80]
10000ebd8:     	add	x8, sp, #0x90
10000ebdc:     	adrp	x9, 0x100038000 <__ZN3std6thread5local17LocalKey$LT$T$GT$8try_with17hafda283f2a50e3b8E+0x58>
10000ebe0:     	add	x9, x9, #0x688
10000ebe4:     	stp	x8, x9, [x29, #-0x60]
10000ebe8:     	sub	x8, x29, #0x80
10000ebec:     	adrp	x9, 0x100122000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
10000ebf0:     	add	x9, x9, #0xc8c
10000ebf4:     	stp	x8, x9, [x29, #-0x50]
10000ebf8:     	adrp	x0, 0x100141000 <_writev+0x100141000>
10000ebfc:     	add	x0, x0, #0xcf
10000ec00:     	sub	x8, x29, #0x78
10000ec04:     	sub	x1, x29, #0x60
10000ec08:     	bl	0x10011927c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
10000ec0c:     	adrp	x1, 0x10012e000 <_aura_data_146+0x16a4>
10000ec10:     	add	x1, x1, #0xf53
10000ec14:     	add	x0, sp, #0x38
10000ec18:     	sub	x2, x29, #0x78
10000ec1c:     	bl	0x10002f00c <__ZN13aura_compiler4diag10Diagnostic5coded17hc0c6d1b6375dce8eE>
10000ec20:     	ldr	x8, [sp, #0x38]
10000ec24:     	cmp	x8, #0x2
10000ec28:     	b.eq	0x10000e994 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x70>
10000ec2c:     	ldur	q0, [sp, #0x40]
10000ec30:     	str	q0, [sp, #0x20]
10000ec34:     	ldr	x9, [sp, #0x50]
10000ec38:     	str	x9, [sp, #0x30]
10000ec3c:     	ldur	q1, [sp, #0x58]
10000ec40:     	ldur	q2, [sp, #0x68]
10000ec44:     	stp	q1, q2, [x19, #0x20]
10000ec48:     	ldur	q1, [sp, #0x78]
10000ec4c:     	str	q1, [x19, #0x40]
10000ec50:     	ldr	x10, [sp, #0x88]
10000ec54:     	str	x10, [x19, #0x50]
10000ec58:     	stur	q0, [x19, #0x8]
10000ec5c:     	str	x9, [x19, #0x18]
10000ec60:     	b	0x10000ef0c <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x5e8>
10000ec64:     	ldur	x8, [x29, #-0x58]
10000ec68:     	b	0x10000e98c <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x68>
10000ec6c:     	mov	x0, sp
10000ec70:     	mov	x2, x22
10000ec74:     	mov	w3, #0x4                ; =4
10000ec78:     	mov	w4, #0x4                ; =4
10000ec7c:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000ec80:     	ldp	x8, x1, [sp, #0x8]
10000ec84:     	cmp	x22, #0x10
10000ec88:     	b.lo	0x10000e9dc <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0xb8>
10000ec8c:     	and	x10, x22, #0x1ffffffffffffff0
10000ec90:     	add	x9, x1, x10
10000ec94:     	add	x11, x21, #0x20
10000ec98:     	add	x12, x20, #0x20
10000ec9c:     	add	x13, x8, x1, lsl #2
10000eca0:     	add	x13, x13, #0x20
10000eca4:     	and	x14, x22, #0x1ffffffffffffff0
10000eca8:     	ldp	q0, q1, [x11, #-0x20]
10000ecac:     	ldp	q2, q3, [x11], #0x40
10000ecb0:     	ldp	q4, q5, [x12, #-0x20]
10000ecb4:     	ldp	q6, q7, [x12], #0x40
10000ecb8:     	fadd.4s	v0, v0, v4
10000ecbc:     	fadd.4s	v1, v1, v5
10000ecc0:     	fadd.4s	v2, v2, v6
10000ecc4:     	fadd.4s	v3, v3, v7
10000ecc8:     	stp	q0, q1, [x13, #-0x20]
10000eccc:     	stp	q2, q3, [x13], #0x40
10000ecd0:     	subs	x14, x14, #0x10
10000ecd4:     	b.ne	0x10000eca8 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x384>
10000ecd8:     	cmp	x22, x10
10000ecdc:     	b.eq	0x10000eef8 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x5d4>
10000ece0:     	lsl	x12, x10, #2
10000ece4:     	add	x11, x21, x12
10000ece8:     	add	x12, x20, x12
10000ecec:     	sub	x10, x22, x10
10000ecf0:     	ldr	s0, [x11], #0x4
10000ecf4:     	ldr	s1, [x12], #0x4
10000ecf8:     	fadd	s0, s0, s1
10000ecfc:     	str	s0, [x8, x9, lsl #2]
10000ed00:     	add	x9, x9, #0x1
10000ed04:     	subs	x10, x10, #0x1
10000ed08:     	b.ne	0x10000ecf0 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x3cc>
10000ed0c:     	b	0x10000eef8 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x5d4>
10000ed10:     	mov	x0, sp
10000ed14:     	mov	x2, x22
10000ed18:     	mov	w3, #0x4                ; =4
10000ed1c:     	mov	w4, #0x4                ; =4
10000ed20:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000ed24:     	ldp	x8, x1, [sp, #0x8]
10000ed28:     	cmp	x22, #0x10
10000ed2c:     	b.lo	0x10000ea1c <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0xf8>
10000ed30:     	and	x10, x22, #0x1ffffffffffffff0
10000ed34:     	add	x9, x1, x10
10000ed38:     	add	x11, x21, #0x20
10000ed3c:     	add	x12, x20, #0x20
10000ed40:     	add	x13, x8, x1, lsl #2
10000ed44:     	add	x13, x13, #0x20
10000ed48:     	and	x14, x22, #0x1ffffffffffffff0
10000ed4c:     	ldp	q0, q1, [x11, #-0x20]
10000ed50:     	ldp	q2, q3, [x11], #0x40
10000ed54:     	ldp	q4, q5, [x12, #-0x20]
10000ed58:     	ldp	q6, q7, [x12], #0x40
10000ed5c:     	fmul.4s	v0, v0, v4
10000ed60:     	fmul.4s	v1, v1, v5
10000ed64:     	fmul.4s	v2, v2, v6
10000ed68:     	fmul.4s	v3, v3, v7
10000ed6c:     	stp	q0, q1, [x13, #-0x20]
10000ed70:     	stp	q2, q3, [x13], #0x40
10000ed74:     	subs	x14, x14, #0x10
10000ed78:     	b.ne	0x10000ed4c <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x428>
10000ed7c:     	cmp	x22, x10
10000ed80:     	b.eq	0x10000eef8 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x5d4>
10000ed84:     	lsl	x12, x10, #2
10000ed88:     	add	x11, x21, x12
10000ed8c:     	add	x12, x20, x12
10000ed90:     	sub	x10, x22, x10
10000ed94:     	ldr	s0, [x11], #0x4
10000ed98:     	ldr	s1, [x12], #0x4
10000ed9c:     	fmul	s0, s0, s1
10000eda0:     	str	s0, [x8, x9, lsl #2]
10000eda4:     	add	x9, x9, #0x1
10000eda8:     	subs	x10, x10, #0x1
10000edac:     	b.ne	0x10000ed94 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x470>
10000edb0:     	b	0x10000eef8 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x5d4>
10000edb4:     	mov	x0, sp
10000edb8:     	mov	x2, x22
10000edbc:     	mov	w3, #0x4                ; =4
10000edc0:     	mov	w4, #0x4                ; =4
10000edc4:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000edc8:     	ldp	x8, x1, [sp, #0x8]
10000edcc:     	cmp	x22, #0x10
10000edd0:     	b.lo	0x10000ea78 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x154>
10000edd4:     	and	x10, x22, #0x1ffffffffffffff0
10000edd8:     	add	x9, x1, x10
10000eddc:     	add	x11, x21, #0x20
10000ede0:     	add	x12, x20, #0x20
10000ede4:     	add	x13, x8, x1, lsl #2
10000ede8:     	add	x13, x13, #0x20
10000edec:     	and	x14, x22, #0x1ffffffffffffff0
10000edf0:     	ldp	q0, q1, [x11, #-0x20]
10000edf4:     	ldp	q2, q3, [x11], #0x40
10000edf8:     	ldp	q4, q5, [x12, #-0x20]
10000edfc:     	ldp	q6, q7, [x12], #0x40
10000ee00:     	fsub.4s	v0, v0, v4
10000ee04:     	fsub.4s	v1, v1, v5
10000ee08:     	fsub.4s	v2, v2, v6
10000ee0c:     	fsub.4s	v3, v3, v7
10000ee10:     	stp	q0, q1, [x13, #-0x20]
10000ee14:     	stp	q2, q3, [x13], #0x40
10000ee18:     	subs	x14, x14, #0x10
10000ee1c:     	b.ne	0x10000edf0 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x4cc>
10000ee20:     	cmp	x22, x10
10000ee24:     	b.eq	0x10000eef8 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x5d4>
10000ee28:     	lsl	x12, x10, #2
10000ee2c:     	add	x11, x21, x12
10000ee30:     	add	x12, x20, x12
10000ee34:     	sub	x10, x22, x10
10000ee38:     	ldr	s0, [x11], #0x4
10000ee3c:     	ldr	s1, [x12], #0x4
10000ee40:     	fsub	s0, s0, s1
10000ee44:     	str	s0, [x8, x9, lsl #2]
10000ee48:     	add	x9, x9, #0x1
10000ee4c:     	subs	x10, x10, #0x1
10000ee50:     	b.ne	0x10000ee38 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x514>
10000ee54:     	b	0x10000eef8 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x5d4>
10000ee58:     	mov	x0, sp
10000ee5c:     	mov	x2, x22
10000ee60:     	mov	w3, #0x4                ; =4
10000ee64:     	mov	w4, #0x4                ; =4
10000ee68:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000ee6c:     	ldp	x8, x1, [sp, #0x8]
10000ee70:     	cmp	x22, #0x10
10000ee74:     	b.lo	0x10000ead4 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x1b0>
10000ee78:     	and	x10, x22, #0x1ffffffffffffff0
10000ee7c:     	add	x9, x1, x10
10000ee80:     	add	x11, x21, #0x20
10000ee84:     	add	x12, x20, #0x20
10000ee88:     	add	x13, x8, x1, lsl #2
10000ee8c:     	add	x13, x13, #0x20
10000ee90:     	and	x14, x22, #0x1ffffffffffffff0
10000ee94:     	ldp	q0, q1, [x11, #-0x20]
10000ee98:     	ldp	q2, q3, [x11], #0x40
10000ee9c:     	ldp	q4, q5, [x12, #-0x20]
10000eea0:     	ldp	q6, q7, [x12], #0x40
10000eea4:     	fdiv.4s	v0, v0, v4
10000eea8:     	fdiv.4s	v1, v1, v5
10000eeac:     	fdiv.4s	v2, v2, v6
10000eeb0:     	fdiv.4s	v3, v3, v7
10000eeb4:     	stp	q0, q1, [x13, #-0x20]
10000eeb8:     	stp	q2, q3, [x13], #0x40
10000eebc:     	subs	x14, x14, #0x10
10000eec0:     	b.ne	0x10000ee94 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x570>
10000eec4:     	cmp	x22, x10
10000eec8:     	b.eq	0x10000eef8 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x5d4>
10000eecc:     	lsl	x12, x10, #2
10000eed0:     	add	x11, x21, x12
10000eed4:     	add	x12, x20, x12
10000eed8:     	sub	x10, x22, x10
10000eedc:     	ldr	s0, [x11], #0x4
10000eee0:     	ldr	s1, [x12], #0x4
10000eee4:     	fdiv	s0, s0, s1
10000eee8:     	str	s0, [x8, x9, lsl #2]
10000eeec:     	add	x9, x9, #0x1
10000eef0:     	subs	x10, x10, #0x1
10000eef4:     	b.ne	0x10000eedc <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x5b8>
10000eef8:     	str	x9, [sp, #0x10]
10000eefc:     	ldr	q0, [sp]
10000ef00:     	stur	q0, [x19, #0x8]
10000ef04:     	str	x9, [x19, #0x18]
10000ef08:     	mov	w8, #0x2                ; =2
10000ef0c:     	str	x8, [x19]
10000ef10:     	ldp	x29, x30, [sp, #0x120]
10000ef14:     	ldp	x20, x19, [sp, #0x110]
10000ef18:     	ldp	x22, x21, [sp, #0x100]
10000ef1c:     	ldp	x24, x23, [sp, #0xf0]
10000ef20:     	ldp	x28, x27, [sp, #0xe0]
10000ef24:     	add	sp, sp, #0x130
10000ef28:     	ret
10000ef2c:     	mov	x19, x0
10000ef30:     	ldr	x8, [sp]
10000ef34:     	cbz	x8, 0x10000ef48 <__ZN13aura_compiler13runtime_value20array_float32_binary17haeb570c846eb5abdE+0x624>
10000ef38:     	ldr	x0, [sp, #0x8]
10000ef3c:     	lsl	x1, x8, #2
10000ef40:     	mov	w2, #0x4                ; =4
10000ef44:     	bl	0x1000631dc <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10000ef48:     	mov	x0, x19
10000ef4c:     	bl	0x10012c1c0 <_writev+0x10012c1c0>

000000010000ef50 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E>:
10000ef50:     	sub	sp, sp, #0x130
10000ef54:     	stp	x28, x27, [sp, #0xe0]
10000ef58:     	stp	x24, x23, [sp, #0xf0]
10000ef5c:     	stp	x22, x21, [sp, #0x100]
10000ef60:     	stp	x20, x19, [sp, #0x110]
10000ef64:     	stp	x29, x30, [sp, #0x120]
10000ef68:     	add	x29, sp, #0x120
10000ef6c:     	mov	x24, x5
10000ef70:     	mov	x22, x4
10000ef74:     	mov	x20, x3
10000ef78:     	mov	x23, x2
10000ef7c:     	mov	x21, x1
10000ef80:     	mov	x19, x0
10000ef84:     	cbz	x6, 0x10000efb0 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x60>
10000ef88:     	adrp	x1, 0x10012e000 <_aura_data_146+0x16a4>
10000ef8c:     	add	x1, x1, #0xf2c
10000ef90:     	adrp	x2, 0x10012f000 <_aura_data_146+0x26a4>
10000ef94:     	add	x2, x2, #0x406
10000ef98:     	add	x0, sp, #0x38
10000ef9c:     	mov	w3, #0x43               ; =67
10000efa0:     	bl	0x10002ef38 <__ZN13aura_compiler4diag10Diagnostic5coded17h0c8f829f44e62944E>
10000efa4:     	ldr	x8, [sp, #0x38]
10000efa8:     	cmp	x8, #0x2
10000efac:     	b.ne	0x10000f054 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x104>
10000efb0:     	cbnz	x23, 0x10000f1cc <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x27c>
10000efb4:     	mov	w8, #0x8                ; =8
10000efb8:     	stp	x23, x8, [sp, #0x40]
10000efbc:     	str	xzr, [sp, #0x50]
10000efc0:     	ldr	x8, [sp, #0x50]
10000efc4:     	ldur	q0, [sp, #0x40]
10000efc8:     	str	q0, [sp]
10000efcc:     	str	x8, [sp, #0x10]
10000efd0:     	cmp	x24, #0x1
10000efd4:     	b.gt	0x10000f014 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0xc4>
10000efd8:     	cmp	x22, x23
10000efdc:     	csel	x22, x22, x23, lo
10000efe0:     	cbnz	x24, 0x10000f080 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x130>
10000efe4:     	ldr	x1, [sp, #0x10]
10000efe8:     	ldr	x8, [sp]
10000efec:     	sub	x8, x8, x1
10000eff0:     	cmp	x22, x8
10000eff4:     	b.hi	0x10000f298 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x348>
10000eff8:     	cbz	x22, 0x10000f10c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x1bc>
10000effc:     	ldr	x8, [sp, #0x8]
10000f000:     	cmp	x22, #0x8
10000f004:     	b.hs	0x10000f2b8 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x368>
10000f008:     	mov	x10, #0x0               ; =0
10000f00c:     	mov	x9, x1
10000f010:     	b	0x10000f30c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x3bc>
10000f014:     	cmp	x24, #0x2
10000f018:     	b.ne	0x10000f0b0 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x160>
10000f01c:     	cmp	x22, x23
10000f020:     	csel	x22, x22, x23, lo
10000f024:     	ldr	x1, [sp, #0x10]
10000f028:     	ldr	x8, [sp]
10000f02c:     	sub	x8, x8, x1
10000f030:     	cmp	x22, x8
10000f034:     	b.hi	0x10000f33c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x3ec>
10000f038:     	cbz	x22, 0x10000f10c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x1bc>
10000f03c:     	ldr	x8, [sp, #0x8]
10000f040:     	cmp	x22, #0x8
10000f044:     	b.hs	0x10000f35c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x40c>
10000f048:     	mov	x10, #0x0               ; =0
10000f04c:     	mov	x9, x1
10000f050:     	b	0x10000f3b0 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x460>
10000f054:     	ldur	q0, [sp, #0x58]
10000f058:     	ldur	q1, [sp, #0x68]
10000f05c:     	stp	q0, q1, [x19, #0x20]
10000f060:     	ldur	q0, [sp, #0x78]
10000f064:     	str	q0, [x19, #0x40]
10000f068:     	ldr	x8, [sp, #0x88]
10000f06c:     	str	x8, [x19, #0x50]
10000f070:     	ldur	q0, [sp, #0x38]
10000f074:     	ldur	q1, [sp, #0x48]
10000f078:     	stp	q0, q1, [x19]
10000f07c:     	b	0x10000f53c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x5ec>
10000f080:     	ldr	x1, [sp, #0x10]
10000f084:     	ldr	x8, [sp]
10000f088:     	sub	x8, x8, x1
10000f08c:     	cmp	x22, x8
10000f090:     	b.hi	0x10000f3e0 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x490>
10000f094:     	cbz	x22, 0x10000f10c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x1bc>
10000f098:     	ldr	x8, [sp, #0x8]
10000f09c:     	cmp	x22, #0x8
10000f0a0:     	b.hs	0x10000f400 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x4b0>
10000f0a4:     	mov	x10, #0x0               ; =0
10000f0a8:     	mov	x9, x1
10000f0ac:     	b	0x10000f454 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x504>
10000f0b0:     	cbz	x22, 0x10000f0d4 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x184>
10000f0b4:     	mov	x8, #0x0                ; =0
10000f0b8:     	lsl	x9, x22, #3
10000f0bc:     	ldr	d0, [x20, x8, lsl #3]
10000f0c0:     	fcmp	d0, #0.0
10000f0c4:     	b.eq	0x10000f114 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x1c4>
10000f0c8:     	add	x8, x8, #0x1
10000f0cc:     	subs	x9, x9, #0x8
10000f0d0:     	b.ne	0x10000f0bc <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x16c>
10000f0d4:     	cmp	x22, x23
10000f0d8:     	csel	x22, x22, x23, lo
10000f0dc:     	ldr	x1, [sp, #0x10]
10000f0e0:     	ldr	x8, [sp]
10000f0e4:     	sub	x8, x8, x1
10000f0e8:     	cmp	x22, x8
10000f0ec:     	b.hi	0x10000f484 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x534>
10000f0f0:     	cbz	x22, 0x10000f10c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x1bc>
10000f0f4:     	ldr	x8, [sp, #0x8]
10000f0f8:     	cmp	x22, #0x8
10000f0fc:     	b.hs	0x10000f4a4 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x554>
10000f100:     	mov	x10, #0x0               ; =0
10000f104:     	mov	x9, x1
10000f108:     	b	0x10000f4f8 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x5a8>
10000f10c:     	mov	x9, x1
10000f110:     	b	0x10000f524 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x5d4>
10000f114:     	str	x8, [sp, #0x90]
10000f118:     	adrp	x8, 0x10012f000 <_aura_data_146+0x26a4>
10000f11c:     	add	x8, x8, #0x21e
10000f120:     	mov	w9, #0x12               ; =18
10000f124:     	stp	x8, x9, [sp, #0x20]
10000f128:     	adrp	x8, 0x100130000 <_aura_data_146+0x36a4>
10000f12c:     	add	x8, x8, #0x998
10000f130:     	mov	w9, #0x8                ; =8
10000f134:     	stp	x8, x9, [x29, #-0x78]
10000f138:     	sub	x8, x29, #0x78
10000f13c:     	adrp	x9, 0x100038000 <__ZN3std6thread5local17LocalKey$LT$T$GT$8try_with17hafda283f2a50e3b8E+0x58>
10000f140:     	add	x9, x9, #0x688
10000f144:     	stp	x8, x9, [sp, #0x38]
10000f148:     	add	x8, sp, #0x20
10000f14c:     	stp	x8, x9, [sp, #0x48]
10000f150:     	add	x8, sp, #0x90
10000f154:     	adrp	x9, 0x100122000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
10000f158:     	add	x9, x9, #0xc8c
10000f15c:     	stp	x8, x9, [sp, #0x58]
10000f160:     	adrp	x0, 0x100141000 <_writev+0x100141000>
10000f164:     	add	x0, x0, #0x103
10000f168:     	sub	x8, x29, #0x60
10000f16c:     	add	x1, sp, #0x38
10000f170:     	bl	0x10011927c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
10000f174:     	adrp	x1, 0x10012f000 <_aura_data_146+0x26a4>
10000f178:     	add	x1, x1, #0x218
10000f17c:     	add	x0, sp, #0x38
10000f180:     	sub	x2, x29, #0x60
10000f184:     	bl	0x10002f00c <__ZN13aura_compiler4diag10Diagnostic5coded17hc0c6d1b6375dce8eE>
10000f188:     	ldur	q0, [sp, #0x58]
10000f18c:     	ldur	q1, [sp, #0x68]
10000f190:     	stp	q0, q1, [x19, #0x20]
10000f194:     	ldur	q0, [sp, #0x78]
10000f198:     	str	q0, [x19, #0x40]
10000f19c:     	ldr	x8, [sp, #0x88]
10000f1a0:     	str	x8, [x19, #0x50]
10000f1a4:     	ldur	q0, [sp, #0x38]
10000f1a8:     	ldur	q1, [sp, #0x48]
10000f1ac:     	stp	q0, q1, [x19]
10000f1b0:     	ldr	x8, [sp]
10000f1b4:     	cbz	x8, 0x10000f53c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x5ec>
10000f1b8:     	ldr	x0, [sp, #0x8]
10000f1bc:     	lsl	x1, x8, #3
10000f1c0:     	mov	w2, #0x8                ; =8
10000f1c4:     	bl	0x1000631dc <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10000f1c8:     	b	0x10000f53c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x5ec>
10000f1cc:     	sub	x0, x29, #0x60
10000f1d0:     	mov	x1, #0x0                ; =0
10000f1d4:     	mov	w2, #0x8                ; =8
10000f1d8:     	mov	x3, x23
10000f1dc:     	mov	w4, #0x8                ; =8
10000f1e0:     	mov	w5, #0x8                ; =8
10000f1e4:     	bl	0x100125708 <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$11finish_grow17h8de8c598a5dc063cE>
10000f1e8:     	ldur	w8, [x29, #-0x60]
10000f1ec:     	tbz	w8, #0x0, 0x10000f290 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x340>
10000f1f0:     	adrp	x8, 0x10012f000 <_aura_data_146+0x26a4>
10000f1f4:     	add	x8, x8, #0x201
10000f1f8:     	mov	w9, #0x17               ; =23
10000f1fc:     	stp	x8, x9, [sp, #0x90]
10000f200:     	stur	x23, [x29, #-0x80]
10000f204:     	add	x8, sp, #0x90
10000f208:     	adrp	x9, 0x100038000 <__ZN3std6thread5local17LocalKey$LT$T$GT$8try_with17hafda283f2a50e3b8E+0x58>
10000f20c:     	add	x9, x9, #0x688
10000f210:     	stp	x8, x9, [x29, #-0x60]
10000f214:     	sub	x8, x29, #0x80
10000f218:     	adrp	x9, 0x100122000 <__RNvXs2_NtNtCsl8K0bEFm1U0_4core3str5lossyNtB5_10Utf8ChunksNtNtNtNtB9_4iter6traits8iterator8Iterator4next+0x124>
10000f21c:     	add	x9, x9, #0xc8c
10000f220:     	stp	x8, x9, [x29, #-0x50]
10000f224:     	adrp	x0, 0x100141000 <_writev+0x100141000>
10000f228:     	add	x0, x0, #0xcf
10000f22c:     	sub	x8, x29, #0x78
10000f230:     	sub	x1, x29, #0x60
10000f234:     	bl	0x10011927c <__RNvNvNtCs1OjIl8oxbrv_5alloc3fmt6format12format_inner>
10000f238:     	adrp	x1, 0x10012e000 <_aura_data_146+0x16a4>
10000f23c:     	add	x1, x1, #0xf53
10000f240:     	add	x0, sp, #0x38
10000f244:     	sub	x2, x29, #0x78
10000f248:     	bl	0x10002f00c <__ZN13aura_compiler4diag10Diagnostic5coded17hc0c6d1b6375dce8eE>
10000f24c:     	ldr	x8, [sp, #0x38]
10000f250:     	cmp	x8, #0x2
10000f254:     	b.eq	0x10000efc0 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x70>
10000f258:     	ldur	q0, [sp, #0x40]
10000f25c:     	str	q0, [sp, #0x20]
10000f260:     	ldr	x9, [sp, #0x50]
10000f264:     	str	x9, [sp, #0x30]
10000f268:     	ldur	q1, [sp, #0x58]
10000f26c:     	ldur	q2, [sp, #0x68]
10000f270:     	stp	q1, q2, [x19, #0x20]
10000f274:     	ldur	q1, [sp, #0x78]
10000f278:     	str	q1, [x19, #0x40]
10000f27c:     	ldr	x10, [sp, #0x88]
10000f280:     	str	x10, [x19, #0x50]
10000f284:     	stur	q0, [x19, #0x8]
10000f288:     	str	x9, [x19, #0x18]
10000f28c:     	b	0x10000f538 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x5e8>
10000f290:     	ldur	x8, [x29, #-0x58]
10000f294:     	b	0x10000efb8 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x68>
10000f298:     	mov	x0, sp
10000f29c:     	mov	x2, x22
10000f2a0:     	mov	w3, #0x8                ; =8
10000f2a4:     	mov	w4, #0x8                ; =8
10000f2a8:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000f2ac:     	ldp	x8, x1, [sp, #0x8]
10000f2b0:     	cmp	x22, #0x8
10000f2b4:     	b.lo	0x10000f008 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0xb8>
10000f2b8:     	and	x10, x22, #0xffffffffffffff8
10000f2bc:     	add	x9, x1, x10
10000f2c0:     	add	x11, x21, #0x20
10000f2c4:     	add	x12, x20, #0x20
10000f2c8:     	add	x13, x8, x1, lsl #3
10000f2cc:     	add	x13, x13, #0x20
10000f2d0:     	and	x14, x22, #0xffffffffffffff8
10000f2d4:     	ldp	q0, q1, [x11, #-0x20]
10000f2d8:     	ldp	q2, q3, [x11], #0x40
10000f2dc:     	ldp	q4, q5, [x12, #-0x20]
10000f2e0:     	ldp	q6, q7, [x12], #0x40
10000f2e4:     	fadd.2d	v0, v0, v4
10000f2e8:     	fadd.2d	v1, v1, v5
10000f2ec:     	fadd.2d	v2, v2, v6
10000f2f0:     	fadd.2d	v3, v3, v7
10000f2f4:     	stp	q0, q1, [x13, #-0x20]
10000f2f8:     	stp	q2, q3, [x13], #0x40
10000f2fc:     	subs	x14, x14, #0x8
10000f300:     	b.ne	0x10000f2d4 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x384>
10000f304:     	cmp	x22, x10
10000f308:     	b.eq	0x10000f524 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x5d4>
10000f30c:     	lsl	x12, x10, #3
10000f310:     	add	x11, x21, x12
10000f314:     	add	x12, x20, x12
10000f318:     	sub	x10, x22, x10
10000f31c:     	ldr	d0, [x11], #0x8
10000f320:     	ldr	d1, [x12], #0x8
10000f324:     	fadd	d0, d0, d1
10000f328:     	str	d0, [x8, x9, lsl #3]
10000f32c:     	add	x9, x9, #0x1
10000f330:     	subs	x10, x10, #0x1
10000f334:     	b.ne	0x10000f31c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x3cc>
10000f338:     	b	0x10000f524 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x5d4>
10000f33c:     	mov	x0, sp
10000f340:     	mov	x2, x22
10000f344:     	mov	w3, #0x8                ; =8
10000f348:     	mov	w4, #0x8                ; =8
10000f34c:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000f350:     	ldp	x8, x1, [sp, #0x8]
10000f354:     	cmp	x22, #0x8
10000f358:     	b.lo	0x10000f048 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0xf8>
10000f35c:     	and	x10, x22, #0xffffffffffffff8
10000f360:     	add	x9, x1, x10
10000f364:     	add	x11, x21, #0x20
10000f368:     	add	x12, x20, #0x20
10000f36c:     	add	x13, x8, x1, lsl #3
10000f370:     	add	x13, x13, #0x20
10000f374:     	and	x14, x22, #0xffffffffffffff8
10000f378:     	ldp	q0, q1, [x11, #-0x20]
10000f37c:     	ldp	q2, q3, [x11], #0x40
10000f380:     	ldp	q4, q5, [x12, #-0x20]
10000f384:     	ldp	q6, q7, [x12], #0x40
10000f388:     	fmul.2d	v0, v0, v4
10000f38c:     	fmul.2d	v1, v1, v5
10000f390:     	fmul.2d	v2, v2, v6
10000f394:     	fmul.2d	v3, v3, v7
10000f398:     	stp	q0, q1, [x13, #-0x20]
10000f39c:     	stp	q2, q3, [x13], #0x40
10000f3a0:     	subs	x14, x14, #0x8
10000f3a4:     	b.ne	0x10000f378 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x428>
10000f3a8:     	cmp	x22, x10
10000f3ac:     	b.eq	0x10000f524 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x5d4>
10000f3b0:     	lsl	x12, x10, #3
10000f3b4:     	add	x11, x21, x12
10000f3b8:     	add	x12, x20, x12
10000f3bc:     	sub	x10, x22, x10
10000f3c0:     	ldr	d0, [x11], #0x8
10000f3c4:     	ldr	d1, [x12], #0x8
10000f3c8:     	fmul	d0, d0, d1
10000f3cc:     	str	d0, [x8, x9, lsl #3]
10000f3d0:     	add	x9, x9, #0x1
10000f3d4:     	subs	x10, x10, #0x1
10000f3d8:     	b.ne	0x10000f3c0 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x470>
10000f3dc:     	b	0x10000f524 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x5d4>
10000f3e0:     	mov	x0, sp
10000f3e4:     	mov	x2, x22
10000f3e8:     	mov	w3, #0x8                ; =8
10000f3ec:     	mov	w4, #0x8                ; =8
10000f3f0:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000f3f4:     	ldp	x8, x1, [sp, #0x8]
10000f3f8:     	cmp	x22, #0x8
10000f3fc:     	b.lo	0x10000f0a4 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x154>
10000f400:     	and	x10, x22, #0xffffffffffffff8
10000f404:     	add	x9, x1, x10
10000f408:     	add	x11, x21, #0x20
10000f40c:     	add	x12, x20, #0x20
10000f410:     	add	x13, x8, x1, lsl #3
10000f414:     	add	x13, x13, #0x20
10000f418:     	and	x14, x22, #0xffffffffffffff8
10000f41c:     	ldp	q0, q1, [x11, #-0x20]
10000f420:     	ldp	q2, q3, [x11], #0x40
10000f424:     	ldp	q4, q5, [x12, #-0x20]
10000f428:     	ldp	q6, q7, [x12], #0x40
10000f42c:     	fsub.2d	v0, v0, v4
10000f430:     	fsub.2d	v1, v1, v5
10000f434:     	fsub.2d	v2, v2, v6
10000f438:     	fsub.2d	v3, v3, v7
10000f43c:     	stp	q0, q1, [x13, #-0x20]
10000f440:     	stp	q2, q3, [x13], #0x40
10000f444:     	subs	x14, x14, #0x8
10000f448:     	b.ne	0x10000f41c <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x4cc>
10000f44c:     	cmp	x22, x10
10000f450:     	b.eq	0x10000f524 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x5d4>
10000f454:     	lsl	x12, x10, #3
10000f458:     	add	x11, x21, x12
10000f45c:     	add	x12, x20, x12
10000f460:     	sub	x10, x22, x10
10000f464:     	ldr	d0, [x11], #0x8
10000f468:     	ldr	d1, [x12], #0x8
10000f46c:     	fsub	d0, d0, d1
10000f470:     	str	d0, [x8, x9, lsl #3]
10000f474:     	add	x9, x9, #0x1
10000f478:     	subs	x10, x10, #0x1
10000f47c:     	b.ne	0x10000f464 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x514>
10000f480:     	b	0x10000f524 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x5d4>
10000f484:     	mov	x0, sp
10000f488:     	mov	x2, x22
10000f48c:     	mov	w3, #0x8                ; =8
10000f490:     	mov	w4, #0x8                ; =8
10000f494:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10000f498:     	ldp	x8, x1, [sp, #0x8]
10000f49c:     	cmp	x22, #0x8
10000f4a0:     	b.lo	0x10000f100 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x1b0>
10000f4a4:     	and	x10, x22, #0xffffffffffffff8
10000f4a8:     	add	x9, x1, x10
10000f4ac:     	add	x11, x21, #0x20
10000f4b0:     	add	x12, x20, #0x20
10000f4b4:     	add	x13, x8, x1, lsl #3
10000f4b8:     	add	x13, x13, #0x20
10000f4bc:     	and	x14, x22, #0xffffffffffffff8
10000f4c0:     	ldp	q0, q1, [x11, #-0x20]
10000f4c4:     	ldp	q2, q3, [x11], #0x40
10000f4c8:     	ldp	q4, q5, [x12, #-0x20]
10000f4cc:     	ldp	q6, q7, [x12], #0x40
10000f4d0:     	fdiv.2d	v0, v0, v4
10000f4d4:     	fdiv.2d	v1, v1, v5
10000f4d8:     	fdiv.2d	v2, v2, v6
10000f4dc:     	fdiv.2d	v3, v3, v7
10000f4e0:     	stp	q0, q1, [x13, #-0x20]
10000f4e4:     	stp	q2, q3, [x13], #0x40
10000f4e8:     	subs	x14, x14, #0x8
10000f4ec:     	b.ne	0x10000f4c0 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x570>
10000f4f0:     	cmp	x22, x10
10000f4f4:     	b.eq	0x10000f524 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x5d4>
10000f4f8:     	lsl	x12, x10, #3
10000f4fc:     	add	x11, x21, x12
10000f500:     	add	x12, x20, x12
10000f504:     	sub	x10, x22, x10
10000f508:     	ldr	d0, [x11], #0x8
10000f50c:     	ldr	d1, [x12], #0x8
10000f510:     	fdiv	d0, d0, d1
10000f514:     	str	d0, [x8, x9, lsl #3]
10000f518:     	add	x9, x9, #0x1
10000f51c:     	subs	x10, x10, #0x1
10000f520:     	b.ne	0x10000f508 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x5b8>
10000f524:     	str	x9, [sp, #0x10]
10000f528:     	ldr	q0, [sp]
10000f52c:     	stur	q0, [x19, #0x8]
10000f530:     	str	x9, [x19, #0x18]
10000f534:     	mov	w8, #0x2                ; =2
10000f538:     	str	x8, [x19]
10000f53c:     	ldp	x29, x30, [sp, #0x120]
10000f540:     	ldp	x20, x19, [sp, #0x110]
10000f544:     	ldp	x22, x21, [sp, #0x100]
10000f548:     	ldp	x24, x23, [sp, #0xf0]
10000f54c:     	ldp	x28, x27, [sp, #0xe0]
10000f550:     	add	sp, sp, #0x130
10000f554:     	ret
10000f558:     	mov	x19, x0
10000f55c:     	ldr	x8, [sp]
10000f560:     	cbz	x8, 0x10000f574 <__ZN13aura_compiler13runtime_value20array_float64_binary17h3c036b23e844f401E+0x624>
10000f564:     	ldr	x0, [sp, #0x8]
10000f568:     	lsl	x1, x8, #3
10000f56c:     	mov	w2, #0x8                ; =8
10000f570:     	bl	0x1000631dc <__RNvCsfLfy6EI15iL_7___rustc14___rust_dealloc>
10000f574:     	mov	x0, x19
10000f578:     	bl	0x10012c1c0 <_writev+0x10012c1c0>

000000010004e9d8 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h42310c836ddc5743E>:
10004e9d8:     	stp	x24, x23, [sp, #-0x40]!
10004e9dc:     	stp	x22, x21, [sp, #0x10]
10004e9e0:     	stp	x20, x19, [sp, #0x20]
10004e9e4:     	stp	x29, x30, [sp, #0x30]
10004e9e8:     	add	x29, sp, #0x30
10004e9ec:     	mov	x20, x1
10004e9f0:     	ldp	x22, x23, [x1]
10004e9f4:     	sub	x24, x23, x22
10004e9f8:     	lsr	x19, x24, #2
10004e9fc:     	ldr	x1, [x0, #0x10]
10004ea00:     	ldr	x8, [x0]
10004ea04:     	sub	x8, x8, x1
10004ea08:     	cmp	x19, x8
10004ea0c:     	b.hi	0x10004eae0 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h42310c836ddc5743E+0x108>
10004ea10:     	cmp	x22, x23
10004ea14:     	b.eq	0x10004ea50 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h42310c836ddc5743E+0x78>
10004ea18:     	ldr	x8, [x0, #0x8]
10004ea1c:     	ldr	x9, [x20, #0x10]
10004ea20:     	cmp	x24, #0x40
10004ea24:     	b.hs	0x10004ea68 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h42310c836ddc5743E+0x90>
10004ea28:     	mov	x10, #0x0               ; =0
10004ea2c:     	sub	x11, x19, x10
10004ea30:     	add	x10, x22, x10, lsl #2
10004ea34:     	ldr	s0, [x10], #0x4
10004ea38:     	ldr	s1, [x9]
10004ea3c:     	fdiv	s0, s0, s1
10004ea40:     	str	s0, [x8, x1, lsl #2]
10004ea44:     	add	x1, x1, #0x1
10004ea48:     	subs	x11, x11, #0x1
10004ea4c:     	b.ne	0x10004ea34 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h42310c836ddc5743E+0x5c>
10004ea50:     	str	x1, [x0, #0x10]
10004ea54:     	ldp	x29, x30, [sp, #0x30]
10004ea58:     	ldp	x20, x19, [sp, #0x20]
10004ea5c:     	ldp	x22, x21, [sp, #0x10]
10004ea60:     	ldp	x24, x23, [sp], #0x40
10004ea64:     	ret
10004ea68:     	mov	x10, #0x0               ; =0
10004ea6c:     	add	x12, x8, x1, lsl #2
10004ea70:     	add	x13, x12, x24
10004ea74:     	add	x11, x9, #0x4
10004ea78:     	cmp	x12, x11
10004ea7c:     	ccmp	x9, x13, #0x2, lo
10004ea80:     	cset	w11, lo
10004ea84:     	cmp	x22, x13
10004ea88:     	ccmp	x12, x23, #0x2, lo
10004ea8c:     	b.lo	0x10004ea2c <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h42310c836ddc5743E+0x54>
10004ea90:     	tbnz	w11, #0x0, 0x10004ea2c <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h42310c836ddc5743E+0x54>
10004ea94:     	and	x10, x19, #0x3ffffffffffffff0
10004ea98:     	ld1r.4s	{ v0 }, [x9]
10004ea9c:     	add	x1, x1, x10
10004eaa0:     	add	x11, x22, #0x20
10004eaa4:     	add	x12, x12, #0x20
10004eaa8:     	and	x13, x19, #0x3ffffffffffffff0
10004eaac:     	ldp	q1, q2, [x11, #-0x20]
10004eab0:     	ldp	q3, q4, [x11], #0x40
10004eab4:     	fdiv.4s	v1, v1, v0
10004eab8:     	fdiv.4s	v2, v2, v0
10004eabc:     	fdiv.4s	v3, v3, v0
10004eac0:     	fdiv.4s	v4, v4, v0
10004eac4:     	stp	q1, q2, [x12, #-0x20]
10004eac8:     	stp	q3, q4, [x12], #0x40
10004eacc:     	subs	x13, x13, #0x10
10004ead0:     	b.ne	0x10004eaac <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h42310c836ddc5743E+0xd4>
10004ead4:     	cmp	x19, x10
10004ead8:     	b.ne	0x10004ea2c <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h42310c836ddc5743E+0x54>
10004eadc:     	b	0x10004ea50 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h42310c836ddc5743E+0x78>
10004eae0:     	mov	x21, x0
10004eae4:     	mov	x2, x19
10004eae8:     	mov	w3, #0x4                ; =4
10004eaec:     	mov	w4, #0x4                ; =4
10004eaf0:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10004eaf4:     	mov	x0, x21
10004eaf8:     	ldr	x1, [x21, #0x10]
10004eafc:     	cmp	x22, x23
10004eb00:     	b.ne	0x10004ea18 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h42310c836ddc5743E+0x40>
10004eb04:     	b	0x10004ea50 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h42310c836ddc5743E+0x78>

000000010004eb08 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h8cdd759e6d87109bE>:
10004eb08:     	stp	x24, x23, [sp, #-0x40]!
10004eb0c:     	stp	x22, x21, [sp, #0x10]
10004eb10:     	stp	x20, x19, [sp, #0x20]
10004eb14:     	stp	x29, x30, [sp, #0x30]
10004eb18:     	add	x29, sp, #0x30
10004eb1c:     	mov	x20, x1
10004eb20:     	ldp	x22, x23, [x1]
10004eb24:     	sub	x24, x23, x22
10004eb28:     	lsr	x19, x24, #3
10004eb2c:     	ldr	x1, [x0, #0x10]
10004eb30:     	ldr	x8, [x0]
10004eb34:     	sub	x8, x8, x1
10004eb38:     	cmp	x19, x8
10004eb3c:     	b.hi	0x10004ec10 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h8cdd759e6d87109bE+0x108>
10004eb40:     	cmp	x22, x23
10004eb44:     	b.eq	0x10004eb80 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h8cdd759e6d87109bE+0x78>
10004eb48:     	ldr	x8, [x0, #0x8]
10004eb4c:     	ldr	x9, [x20, #0x10]
10004eb50:     	cmp	x24, #0x50
10004eb54:     	b.hs	0x10004eb98 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h8cdd759e6d87109bE+0x90>
10004eb58:     	mov	x10, #0x0               ; =0
10004eb5c:     	sub	x11, x19, x10
10004eb60:     	add	x10, x22, x10, lsl #3
10004eb64:     	ldr	d0, [x10], #0x8
10004eb68:     	ldr	d1, [x9]
10004eb6c:     	fdiv	d0, d0, d1
10004eb70:     	str	d0, [x8, x1, lsl #3]
10004eb74:     	add	x1, x1, #0x1
10004eb78:     	subs	x11, x11, #0x1
10004eb7c:     	b.ne	0x10004eb64 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h8cdd759e6d87109bE+0x5c>
10004eb80:     	str	x1, [x0, #0x10]
10004eb84:     	ldp	x29, x30, [sp, #0x30]
10004eb88:     	ldp	x20, x19, [sp, #0x20]
10004eb8c:     	ldp	x22, x21, [sp, #0x10]
10004eb90:     	ldp	x24, x23, [sp], #0x40
10004eb94:     	ret
10004eb98:     	mov	x10, #0x0               ; =0
10004eb9c:     	add	x12, x8, x1, lsl #3
10004eba0:     	add	x13, x12, x24
10004eba4:     	add	x11, x9, #0x8
10004eba8:     	cmp	x12, x11
10004ebac:     	ccmp	x9, x13, #0x2, lo
10004ebb0:     	cset	w11, lo
10004ebb4:     	cmp	x22, x13
10004ebb8:     	ccmp	x12, x23, #0x2, lo
10004ebbc:     	b.lo	0x10004eb5c <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h8cdd759e6d87109bE+0x54>
10004ebc0:     	tbnz	w11, #0x0, 0x10004eb5c <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h8cdd759e6d87109bE+0x54>
10004ebc4:     	and	x10, x19, #0x1ffffffffffffff8
10004ebc8:     	ld1r.2d	{ v0 }, [x9]
10004ebcc:     	add	x1, x1, x10
10004ebd0:     	add	x11, x22, #0x20
10004ebd4:     	add	x12, x12, #0x20
10004ebd8:     	and	x13, x19, #0x1ffffffffffffff8
10004ebdc:     	ldp	q1, q2, [x11, #-0x20]
10004ebe0:     	ldp	q3, q4, [x11], #0x40
10004ebe4:     	fdiv.2d	v1, v1, v0
10004ebe8:     	fdiv.2d	v2, v2, v0
10004ebec:     	fdiv.2d	v3, v3, v0
10004ebf0:     	fdiv.2d	v4, v4, v0
10004ebf4:     	stp	q1, q2, [x12, #-0x20]
10004ebf8:     	stp	q3, q4, [x12], #0x40
10004ebfc:     	subs	x13, x13, #0x8
10004ec00:     	b.ne	0x10004ebdc <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h8cdd759e6d87109bE+0xd4>
10004ec04:     	cmp	x19, x10
10004ec08:     	b.ne	0x10004eb5c <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h8cdd759e6d87109bE+0x54>
10004ec0c:     	b	0x10004eb80 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h8cdd759e6d87109bE+0x78>
10004ec10:     	mov	x21, x0
10004ec14:     	mov	x2, x19
10004ec18:     	mov	w3, #0x8                ; =8
10004ec1c:     	mov	w4, #0x8                ; =8
10004ec20:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10004ec24:     	mov	x0, x21
10004ec28:     	ldr	x1, [x21, #0x10]
10004ec2c:     	cmp	x22, x23
10004ec30:     	b.ne	0x10004eb48 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h8cdd759e6d87109bE+0x40>
10004ec34:     	b	0x10004eb80 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17h8cdd759e6d87109bE+0x78>

000000010004ec38 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hc4092ea7f1480ea8E>:
10004ec38:     	stp	x24, x23, [sp, #-0x40]!
10004ec3c:     	stp	x22, x21, [sp, #0x10]
10004ec40:     	stp	x20, x19, [sp, #0x20]
10004ec44:     	stp	x29, x30, [sp, #0x30]
10004ec48:     	add	x29, sp, #0x30
10004ec4c:     	mov	x20, x1
10004ec50:     	ldp	x22, x23, [x1]
10004ec54:     	sub	x24, x23, x22
10004ec58:     	lsr	x19, x24, #3
10004ec5c:     	ldr	x1, [x0, #0x10]
10004ec60:     	ldr	x8, [x0]
10004ec64:     	sub	x8, x8, x1
10004ec68:     	cmp	x19, x8
10004ec6c:     	b.hi	0x10004ed40 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hc4092ea7f1480ea8E+0x108>
10004ec70:     	cmp	x22, x23
10004ec74:     	b.eq	0x10004ecb0 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hc4092ea7f1480ea8E+0x78>
10004ec78:     	ldr	x8, [x0, #0x8]
10004ec7c:     	ldr	x9, [x20, #0x10]
10004ec80:     	cmp	x24, #0x50
10004ec84:     	b.hs	0x10004ecc8 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hc4092ea7f1480ea8E+0x90>
10004ec88:     	mov	x10, #0x0               ; =0
10004ec8c:     	sub	x11, x19, x10
10004ec90:     	add	x10, x22, x10, lsl #3
10004ec94:     	ldr	d0, [x10], #0x8
10004ec98:     	ldr	d1, [x9]
10004ec9c:     	fdiv	d0, d1, d0
10004eca0:     	str	d0, [x8, x1, lsl #3]
10004eca4:     	add	x1, x1, #0x1
10004eca8:     	subs	x11, x11, #0x1
10004ecac:     	b.ne	0x10004ec94 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hc4092ea7f1480ea8E+0x5c>
10004ecb0:     	str	x1, [x0, #0x10]
10004ecb4:     	ldp	x29, x30, [sp, #0x30]
10004ecb8:     	ldp	x20, x19, [sp, #0x20]
10004ecbc:     	ldp	x22, x21, [sp, #0x10]
10004ecc0:     	ldp	x24, x23, [sp], #0x40
10004ecc4:     	ret
10004ecc8:     	mov	x10, #0x0               ; =0
10004eccc:     	add	x12, x8, x1, lsl #3
10004ecd0:     	add	x13, x12, x24
10004ecd4:     	add	x11, x9, #0x8
10004ecd8:     	cmp	x12, x11
10004ecdc:     	ccmp	x9, x13, #0x2, lo
10004ece0:     	cset	w11, lo
10004ece4:     	cmp	x22, x13
10004ece8:     	ccmp	x12, x23, #0x2, lo
10004ecec:     	b.lo	0x10004ec8c <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hc4092ea7f1480ea8E+0x54>
10004ecf0:     	tbnz	w11, #0x0, 0x10004ec8c <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hc4092ea7f1480ea8E+0x54>
10004ecf4:     	and	x10, x19, #0x1ffffffffffffff8
10004ecf8:     	ld1r.2d	{ v0 }, [x9]
10004ecfc:     	add	x1, x1, x10
10004ed00:     	add	x11, x22, #0x20
10004ed04:     	add	x12, x12, #0x20
10004ed08:     	and	x13, x19, #0x1ffffffffffffff8
10004ed0c:     	ldp	q1, q2, [x11, #-0x20]
10004ed10:     	ldp	q3, q4, [x11], #0x40
10004ed14:     	fdiv.2d	v1, v0, v1
10004ed18:     	fdiv.2d	v2, v0, v2
10004ed1c:     	fdiv.2d	v3, v0, v3
10004ed20:     	fdiv.2d	v4, v0, v4
10004ed24:     	stp	q1, q2, [x12, #-0x20]
10004ed28:     	stp	q3, q4, [x12], #0x40
10004ed2c:     	subs	x13, x13, #0x8
10004ed30:     	b.ne	0x10004ed0c <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hc4092ea7f1480ea8E+0xd4>
10004ed34:     	cmp	x19, x10
10004ed38:     	b.ne	0x10004ec8c <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hc4092ea7f1480ea8E+0x54>
10004ed3c:     	b	0x10004ecb0 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hc4092ea7f1480ea8E+0x78>
10004ed40:     	mov	x21, x0
10004ed44:     	mov	x2, x19
10004ed48:     	mov	w3, #0x8                ; =8
10004ed4c:     	mov	w4, #0x8                ; =8
10004ed50:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10004ed54:     	mov	x0, x21
10004ed58:     	ldr	x1, [x21, #0x10]
10004ed5c:     	cmp	x22, x23
10004ed60:     	b.ne	0x10004ec78 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hc4092ea7f1480ea8E+0x40>
10004ed64:     	b	0x10004ecb0 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hc4092ea7f1480ea8E+0x78>

000000010004ed68 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hfd737092eb9c7864E>:
10004ed68:     	stp	x24, x23, [sp, #-0x40]!
10004ed6c:     	stp	x22, x21, [sp, #0x10]
10004ed70:     	stp	x20, x19, [sp, #0x20]
10004ed74:     	stp	x29, x30, [sp, #0x30]
10004ed78:     	add	x29, sp, #0x30
10004ed7c:     	mov	x20, x1
10004ed80:     	ldp	x22, x23, [x1]
10004ed84:     	sub	x24, x23, x22
10004ed88:     	lsr	x19, x24, #2
10004ed8c:     	ldr	x1, [x0, #0x10]
10004ed90:     	ldr	x8, [x0]
10004ed94:     	sub	x8, x8, x1
10004ed98:     	cmp	x19, x8
10004ed9c:     	b.hi	0x10004ee70 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hfd737092eb9c7864E+0x108>
10004eda0:     	cmp	x22, x23
10004eda4:     	b.eq	0x10004ede0 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hfd737092eb9c7864E+0x78>
10004eda8:     	ldr	x8, [x0, #0x8]
10004edac:     	ldr	x9, [x20, #0x10]
10004edb0:     	cmp	x24, #0x40
10004edb4:     	b.hs	0x10004edf8 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hfd737092eb9c7864E+0x90>
10004edb8:     	mov	x10, #0x0               ; =0
10004edbc:     	sub	x11, x19, x10
10004edc0:     	add	x10, x22, x10, lsl #2
10004edc4:     	ldr	s0, [x10], #0x4
10004edc8:     	ldr	s1, [x9]
10004edcc:     	fdiv	s0, s1, s0
10004edd0:     	str	s0, [x8, x1, lsl #2]
10004edd4:     	add	x1, x1, #0x1
10004edd8:     	subs	x11, x11, #0x1
10004eddc:     	b.ne	0x10004edc4 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hfd737092eb9c7864E+0x5c>
10004ede0:     	str	x1, [x0, #0x10]
10004ede4:     	ldp	x29, x30, [sp, #0x30]
10004ede8:     	ldp	x20, x19, [sp, #0x20]
10004edec:     	ldp	x22, x21, [sp, #0x10]
10004edf0:     	ldp	x24, x23, [sp], #0x40
10004edf4:     	ret
10004edf8:     	mov	x10, #0x0               ; =0
10004edfc:     	add	x12, x8, x1, lsl #2
10004ee00:     	add	x13, x12, x24
10004ee04:     	add	x11, x9, #0x4
10004ee08:     	cmp	x12, x11
10004ee0c:     	ccmp	x9, x13, #0x2, lo
10004ee10:     	cset	w11, lo
10004ee14:     	cmp	x22, x13
10004ee18:     	ccmp	x12, x23, #0x2, lo
10004ee1c:     	b.lo	0x10004edbc <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hfd737092eb9c7864E+0x54>
10004ee20:     	tbnz	w11, #0x0, 0x10004edbc <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hfd737092eb9c7864E+0x54>
10004ee24:     	and	x10, x19, #0x3ffffffffffffff0
10004ee28:     	ld1r.4s	{ v0 }, [x9]
10004ee2c:     	add	x1, x1, x10
10004ee30:     	add	x11, x22, #0x20
10004ee34:     	add	x12, x12, #0x20
10004ee38:     	and	x13, x19, #0x3ffffffffffffff0
10004ee3c:     	ldp	q1, q2, [x11, #-0x20]
10004ee40:     	ldp	q3, q4, [x11], #0x40
10004ee44:     	fdiv.4s	v1, v0, v1
10004ee48:     	fdiv.4s	v2, v0, v2
10004ee4c:     	fdiv.4s	v3, v0, v3
10004ee50:     	fdiv.4s	v4, v0, v4
10004ee54:     	stp	q1, q2, [x12, #-0x20]
10004ee58:     	stp	q3, q4, [x12], #0x40
10004ee5c:     	subs	x13, x13, #0x10
10004ee60:     	b.ne	0x10004ee3c <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hfd737092eb9c7864E+0xd4>
10004ee64:     	cmp	x19, x10
10004ee68:     	b.ne	0x10004edbc <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hfd737092eb9c7864E+0x54>
10004ee6c:     	b	0x10004ede0 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hfd737092eb9c7864E+0x78>
10004ee70:     	mov	x21, x0
10004ee74:     	mov	x2, x19
10004ee78:     	mov	w3, #0x4                ; =4
10004ee7c:     	mov	w4, #0x4                ; =4
10004ee80:     	bl	0x1001257bc <__ZN5alloc7raw_vec20RawVecInner$LT$A$GT$7reserve21do_reserve_and_handle17h91a2f0b9ed19325bE>
10004ee84:     	mov	x0, x21
10004ee88:     	ldr	x1, [x21, #0x10]
10004ee8c:     	cmp	x22, x23
10004ee90:     	b.ne	0x10004eda8 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hfd737092eb9c7864E+0x40>
10004ee94:     	b	0x10004ede0 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$14extend_trusted17hfd737092eb9c7864E+0x78>
