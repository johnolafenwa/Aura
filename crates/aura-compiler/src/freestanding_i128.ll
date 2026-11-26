; Runtime-free 128-bit arithmetic. The caller checks the returned trap code.
; The internal divider shifts in a dividend bit at each step. Before the
; last iteration the partial remainder fits in 127 bits.
define internal {i128, i128} @aura.systems.udivrem(i128 %a, i128 %b) {
entry:
  br label %loop
loop:
  %i = phi i32 [0, %entry], [%next, %loop]
  %q = phi i128 [%a, %entry], [%nq, %loop]
  %r = phi i128 [0, %entry], [%nr, %loop]
  %top = lshr i128 %q, 127
  %twice = shl i128 %r, 1
  %candidate = or i128 %twice, %top
  %fits = icmp uge i128 %candidate, %b
  %sub = sub i128 %candidate, %b
  %nr = select i1 %fits, i128 %sub, i128 %candidate
  %bit = zext i1 %fits to i128
  %q2 = shl i128 %q, 1
  %nq = or i128 %q2, %bit
  %next = add i32 %i, 1
  %done = icmp eq i32 %next, 128
  br i1 %done, label %end, label %loop
end:
  %pair = insertvalue {i128,i128} poison, i128 %nq, 0
  %complete = insertvalue {i128,i128} %pair, i128 %nr, 1
  ret {i128,i128} %complete
}

declare i128 @llvm.ctpop.i128(i128)
declare i128 @llvm.ctlz.i128(i128, i1 immarg)
declare i128 @llvm.cttz.i128(i128, i1 immarg)
declare i128 @llvm.bswap.i128(i128)
declare i128 @llvm.fshl.i128(i128, i128, i128)
declare i128 @llvm.fshr.i128(i128, i128, i128)

define {i64, i64, i64} @aura.systems.integer128(i64 %op, i64 %signed, i64 %al, i64 %ah, i64 %bl, i64 %bh) alwaysinline {
entry:
  %ahl = zext i64 %ah to i128
  %ahh = shl i128 %ahl, 64
  %all = zext i64 %al to i128
  %a = or i128 %all, %ahh
  %bhl = zext i64 %bh to i128
  %bhh = shl i128 %bhl, 64
  %bll = zext i64 %bl to i128
  %b = or i128 %bll, %bhh
  %s = icmp ne i64 %signed, 0
  %na0 = icmp slt i128 %a, 0
  %nb0 = icmp slt i128 %b, 0
  %na = and i1 %s, %na0
  %nb = and i1 %s, %nb0
  %negative = xor i1 %na, %nb
  %ma0 = sub i128 0, %a
  %mb0 = sub i128 0, %b
  %ma = select i1 %na, i128 %ma0, i128 %a
  %mb = select i1 %nb, i128 %mb0, i128 %b
  %limit = select i1 %negative, i128 170141183460469231731687303715884105728, i128 170141183460469231731687303715884105727
  %bound = select i1 %s, i128 %limit, i128 -1
  switch i64 %op, label %invalid [
    i64 0, label %add
    i64 1, label %sub
    i64 2, label %mul
    i64 3, label %divmod
    i64 4, label %divmod
    i64 5, label %and
    i64 6, label %or
    i64 7, label %xor
    i64 8, label %shl
    i64 9, label %shr
    i64 10, label %rol
    i64 11, label %ror
    i64 12, label %pop
    i64 13, label %clz
    i64 14, label %ctz
    i64 15, label %bswap
    i64 16, label %eq
    i64 17, label %ne
    i64 18, label %lt
    i64 19, label %le
    i64 20, label %gt
    i64 21, label %ge
    i64 22, label %divmod
    i64 23, label %fromfp
    i64 24, label %tofp
    i64 25, label %tofp
    i64 26, label %tofp
    i64 27, label %absolute
  ]
add:
  %sum = add i128 %a, %b
  %uadd = icmp ult i128 %sum, %a
  %ad1 = xor i128 %a, %sum
  %ad2 = xor i128 %b, %sum
  %ad3 = and i128 %ad1, %ad2
  %sadd = icmp slt i128 %ad3, 0
  %addfail = select i1 %s, i1 %sadd, i1 %uadd
  br i1 %addfail, label %overflow, label %end
sub:
  %diff = sub i128 %a, %b
  %usub = icmp ult i128 %a, %b
  %sd1 = xor i128 %a, %b
  %sd2 = xor i128 %a, %diff
  %sd3 = and i128 %sd1, %sd2
  %ssub = icmp slt i128 %sd3, 0
  %subfail = select i1 %s, i1 %ssub, i1 %usub
  br i1 %subfail, label %overflow, label %end
mul:
  ; Split the magnitudes at 64 bits. The overflow test uses 64x64 products,
  ; so no target needs a compiler helper for checked multiplication.
  %mask = zext i64 -1 to i128
  %a0 = and i128 %ma, %mask
  %a1 = lshr i128 %ma, 64
  %b0 = and i128 %mb, %mask
  %b1 = lshr i128 %mb, 64
  %prod = mul i128 %ma, %mb
  %p00 = mul i128 %a0, %b0
  %p01 = mul i128 %a0, %b1
  %p10 = mul i128 %a1, %b0
  %carry = lshr i128 %p00, 64
  %small1 = and i128 %p01, %mask
  %small2 = and i128 %p10, %mask
  %mid1 = add i128 %carry, %small1
  %mid2 = add i128 %mid1, %small2
  %full1 = or i128 %p01, %p10
  %full2 = or i128 %full1, %mid2
  %large = icmp ugt i128 %full2, %mask
  %a_high = icmp ne i128 %a1, 0
  %b_high = icmp ne i128 %b1, 0
  %both_high = and i1 %a_high, %b_high
  %big = or i1 %both_high, %large
  %exceeds = icmp ugt i128 %prod, %bound
  %mulbad = or i1 %exceeds, %big
  %inv = sub i128 0, %prod
  %product = select i1 %negative, i128 %inv, i128 %prod
  br i1 %mulbad, label %overflow, label %end
divmod:
  %zero = icmp eq i128 %b, 0
  br i1 %zero, label %divide_zero, label %divide
divide:
  %qr = call {i128,i128} @aura.systems.udivrem(i128 %ma, i128 %mb)
  %q = extractvalue {i128,i128} %qr, 0
  %r = extractvalue {i128,i128} %qr, 1
  %rnonzero = icmp ne i128 %r, 0
  %adjust = and i1 %negative, %rnonzero
  %trunc = icmp eq i64 %op, 22
  %flooradj = select i1 %trunc, i1 false, i1 %adjust
  %adj = zext i1 %flooradj to i128
  %qq = add i128 %q, %adj
  %qneg = sub i128 0, %qq
  %quot = select i1 %negative, i128 %qneg, i128 %q
  %radj = sub i128 %mb, %r
  %rmag = select i1 %adjust, i128 %radj, i128 %r
  %rneg = sub i128 0, %rmag
  %rem = select i1 %nb, i128 %rneg, i128 %rmag
  %isdiv = icmp ne i64 %op, 4
  %divresult = select i1 %isdiv, i128 %quot, i128 %rem
  %qbig = icmp ugt i128 %qq, %bound
  %divbad = and i1 %isdiv, %qbig
  br i1 %divbad, label %overflow, label %end
and:
  %andv = and i128 %a, %b
  br label %end
or:
  %orv = or i128 %a, %b
  br label %end
xor:
  %xorv = xor i128 %a, %b
  br label %end
shl:
  %leftok = icmp ult i128 %b, 128
  br i1 %leftok, label %left, label %shift_error
left:
  %sl = shl i128 %a, %b
  %backu = lshr i128 %sl, %b
  %backs = ashr i128 %sl, %b
  %back = select i1 %s, i128 %backs, i128 %backu
  %leftbad = icmp ne i128 %back, %a
  br i1 %leftbad, label %overflow, label %end
shr:
  %rightok = icmp ult i128 %b, 128
  br i1 %rightok, label %right, label %shift_error
right:
  %sru = lshr i128 %a, %b
  %srs = ashr i128 %a, %b
  %sr = select i1 %s, i128 %srs, i128 %sru
  br label %end
rol:
  %rl = call i128 @llvm.fshl.i128(i128 %a, i128 %a, i128 %b)
  br i1 %nb0, label %shift_error, label %end
ror:
  %rr = call i128 @llvm.fshr.i128(i128 %a, i128 %a, i128 %b)
  br i1 %nb0, label %shift_error, label %end
pop:
  %popv = call i128 @llvm.ctpop.i128(i128 %a)
  br label %end
clz:
  %clzv = call i128 @llvm.ctlz.i128(i128 %a, i1 false)
  br label %end
ctz:
  %ctzv = call i128 @llvm.cttz.i128(i128 %a, i1 false)
  br label %end
bswap:
  %swapped = call i128 @llvm.bswap.i128(i128 %a)
  br label %end
eq:
  %eqflag = icmp eq i128 %a, %b
  %eqv = zext i1 %eqflag to i128
  br label %end
ne:
  %neflag = icmp ne i128 %a, %b
  %nev = zext i1 %neflag to i128
  br label %end
lt:
  %lts = icmp slt i128 %a, %b
  %ltu = icmp ult i128 %a, %b
  %ltflag = select i1 %s, i1 %lts, i1 %ltu
  %ltv = zext i1 %ltflag to i128
  br label %end
le:
  %les = icmp sle i128 %a, %b
  %leu = icmp ule i128 %a, %b
  %leflag = select i1 %s, i1 %les, i1 %leu
  %lev = zext i1 %leflag to i128
  br label %end
gt:
  %gts = icmp sgt i128 %a, %b
  %gtu = icmp ugt i128 %a, %b
  %gtflag = select i1 %s, i1 %gts, i1 %gtu
  %gtv = zext i1 %gtflag to i128
  br label %end
ge:
  %ges = icmp sge i128 %a, %b
  %geu = icmp uge i128 %a, %b
  %geflag = select i1 %s, i1 %ges, i1 %geu
  %gev = zext i1 %geflag to i128
  br label %end
absolute:
  %absbad = icmp slt i128 %ma, 0
  %absfail = and i1 %s, %absbad
  br i1 %absfail, label %overflow, label %end
fromfp:
  ; Decompose the IEEE binary64 exponent and significand before truncation.
  %fpneg = icmp slt i64 %al, 0
  %expfield = lshr i64 %al, 52
  %expbias = and i64 %expfield, 2047
  %tiny = icmp ult i64 %expbias, 1023
  br i1 %tiny, label %fpzero, label %fromfp_check
fromfp_check:
  %exp = sub i64 %expbias, 1023
  %huge = icmp ugt i64 %exp, 127
  %positive = xor i1 %fpneg, true
  %allow = or i1 %positive, %s
  %signbad = xor i1 %allow, true
  %fpbad = or i1 %huge, %signbad
  br i1 %fpbad, label %cast_error, label %fromfp_value
fromfp_value:
  %mant = and i64 %al, 4503599627370495
  %mantissa = or i64 %mant, 4503599627370496
  %m = zext i64 %mantissa to i128
  %l = sub i64 %exp, 52
  %rshift = sub i64 52, %exp
  %lmasked = and i64 %l, 127
  %rmasked = and i64 %rshift, 127
  %lx = zext i64 %lmasked to i128
  %rx = zext i64 %rmasked to i128
  %lm = shl i128 %m, %lx
  %rm = lshr i128 %m, %rx
  %bigexp = icmp uge i64 %exp, 52
  %mag = select i1 %bigexp, i128 %lm, i128 %rm
  %invfp = sub i128 0, %mag
  %intval = select i1 %fpneg, i128 %invfp, i128 %mag
  %destlimit = select i1 %fpneg, i128 170141183460469231731687303715884105728, i128 170141183460469231731687303715884105727
  %outofrange = icmp ugt i128 %mag, %destlimit
  %fbad = and i1 %s, %outofrange
  br i1 %fbad, label %cast_error, label %end
tofp:
  %iszero = icmp eq i128 %ma, 0
  br i1 %iszero, label %fpzero, label %tofp_value
tofp_value:
  ; Keep the high 53 significant bits, rounding halfway values to even.
  %z = call i128 @llvm.ctlz.i128(i128 %ma, i1 false)
  %tz = call i128 @llvm.cttz.i128(i128 %ma, i1 false)
  %exponent = sub i128 127, %z
  %span = sub i128 %exponent, %tz
  %single = icmp eq i64 %op, 25
  %maxspan = select i1 %single, i128 23, i128 52
  %inexact = icmp ugt i128 %span, %maxspan
  %exactonly = icmp ne i64 %op, 26
  %roundbad = and i1 %inexact, %exactonly
  br i1 %roundbad, label %cast_error, label %rounded
rounded:
  %normalized = shl i128 %ma, %z
  %sig = lshr i128 %normalized, 75
  %allfrac = and i128 %normalized, 37778931862957161709567
  %above = icmp ugt i128 %allfrac, 18889465931478580854784
  %half = icmp eq i128 %allfrac, 18889465931478580854784
  %odd = trunc i128 %sig to i1
  %tie = and i1 %half, %odd
  %up = or i1 %above, %tie
  %roundbit = zext i1 %up to i128
  %rounded_sig = add i128 %sig, %roundbit
  ; The hidden one adds one to the biased exponent, including a carry
  ; when rounding overflows the significand.
  %adjustedexp = add i128 %exponent, 1022
  %expbits = shl i128 %adjustedexp, 52
  %fpbits = add i128 %expbits, %rounded_sig
  %signbit = select i1 %na, i128 9223372036854775808, i128 0
  %ieee = or i128 %fpbits, %signbit
  br label %end
fpzero:
  br label %end
cast_error:
  br label %fail
overflow:
  br label %fail
divide_zero:
  br label %fail
shift_error:
  br label %fail
invalid:
  unreachable
fail:
  %trap = phi i64 [1, %overflow], [2, %divide_zero], [6, %shift_error], [5, %cast_error]
  %error = insertvalue {i64,i64,i64} zeroinitializer, i64 %trap, 2
  ret {i64,i64,i64} %error
end:
  %value = phi i128 [ %sum, %add ],
    [ %diff, %sub ],
    [ %product, %mul ],
    [ %divresult, %divide ],
    [ %andv, %and ],
    [ %orv, %or ],
    [ %xorv, %xor ],
    [ %sl, %left ],
    [ %sr, %right ],
    [ %rl, %rol ],
    [ %rr, %ror ],
    [ %popv, %pop ],
    [ %clzv, %clz ],
    [ %ctzv, %ctz ],
    [ %swapped, %bswap ],
    [ %eqv, %eq ],
    [ %nev, %ne ],
    [ %ltv, %lt ],
    [ %lev, %le ],
    [ %gtv, %gt ],
    [ %gev, %ge ], [ %ma, %absolute ], [ %intval, %fromfp_value ],
    [ %ieee, %rounded ], [ 0, %fpzero ]
  %lo = trunc i128 %value to i64
  %upper = lshr i128 %value, 64
  %hi = trunc i128 %upper to i64
  %first = insertvalue {i64,i64,i64} zeroinitializer, i64 %lo, 0
  %result = insertvalue {i64,i64,i64} %first, i64 %hi, 1
  ret {i64,i64,i64} %result
}
