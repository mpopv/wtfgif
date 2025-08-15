;; wasm/color-map.wat
;; Minimal Wasm module for fast palette index → RGBA32 mapping
;; Scalar implementation (SIMD-ready for later extension)
(module
  (memory (export "mem") 1)            ;; ~64KiB to start; grow if needed
  
  ;; map32: Convert n palette indices to n RGBA32 pixels
  ;; param $idx: pointer to input indices (Uint8Array)
  ;; param $out: pointer to output pixels (Uint32Array)  
  ;; param $pal: pointer to palette (Uint32Array, 256 entries)
  ;; param $n: number of pixels to convert
  (func (export "map32")
    (param $idx i32) (param $out i32) (param $pal i32) (param $n i32)
    (local $i i32) (local $c i32) (local $p i32)
    
    (local.set $i (i32.const 0))
    (block $done
      (loop $loop
        ;; Exit if we've processed all pixels
        (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
        
        ;; c = idx[i] (load palette index, extend to 32-bit)
        (local.set $c (i32.extend8_u
          (i32.load8_u (i32.add (local.get $idx) (local.get $i)))))
        
        ;; p = pal + (c<<2) (calculate palette entry address)
        (local.set $p (i32.add (local.get $pal)
                               (i32.shl (local.get $c) (i32.const 2))))
        
        ;; out[i] = pal[c] (store 32-bit color)
        (i32.store
          (i32.add (local.get $out) (i32.shl (local.get $i) (i32.const 2)))
          (i32.load (local.get $p)))
        
        ;; i++
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )
  )
  
  ;; map32_masked: Convert indices to pixels, skipping transparent pixels
  ;; param $idx: pointer to input indices
  ;; param $out: pointer to output pixels
  ;; param $pal: pointer to palette
  ;; param $mask: pointer to transparency mask (1=opaque, 0=transparent)
  ;; param $n: number of pixels
  (func (export "map32_masked")
    (param $idx i32) (param $out i32) (param $pal i32) (param $mask i32) (param $n i32)
    (local $i i32) (local $c i32) (local $p i32) (local $m i32)
    
    (local.set $i (i32.const 0))
    (block $done
      (loop $loop
        (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
        
        ;; Load mask for this pixel
        (local.set $m (i32.extend8_u
          (i32.load8_u (i32.add (local.get $mask) (local.get $i)))))
        
        ;; Only process if mask is non-zero (opaque)
        (if (local.get $m)
          (then
            ;; c = idx[i]
            (local.set $c (i32.extend8_u
              (i32.load8_u (i32.add (local.get $idx) (local.get $i)))))
            
            ;; p = pal + (c<<2)
            (local.set $p (i32.add (local.get $pal)
                                   (i32.shl (local.get $c) (i32.const 2))))
            
            ;; out[i] = pal[c]
            (i32.store
              (i32.add (local.get $out) (i32.shl (local.get $i) (i32.const 2)))
              (i32.load (local.get $p)))
          )
        )
        
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )
  )
)