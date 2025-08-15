#!/bin/bash
# Build script for wtfgif WebAssembly module with SIMD + threading

set -e

echo "🔧 Building wtfgif WebAssembly module..."

# Check if Rust and wasm32 target are installed
if ! command -v cargo &> /dev/null; then
    echo "❌ Cargo not found. Please install Rust: https://rustup.rs/"
    exit 1
fi

if ! rustup target list --installed | grep -q wasm32-unknown-unknown; then
    echo "📥 Installing wasm32-unknown-unknown target..."
    rustup target add wasm32-unknown-unknown
fi

# Build with optimizations and SIMD support
echo "🚀 Compiling Rust to WebAssembly..."
cd "$(dirname "$0")" # Ensure we're in the script directory
RUSTFLAGS='-C target-cpu=generic -C target-feature=+simd128,+atomics,+bulk-memory -C opt-level=3' \
    cargo build --release --target wasm32-unknown-unknown

# Check if wasm-opt is available for further optimization
if command -v wasm-opt &> /dev/null; then
    echo "⚡ Optimizing with wasm-opt..."
    wasm-opt -O3 --enable-simd --enable-threads \
        target/wasm32-unknown-unknown/release/wtfgif.wasm \
        -o ../wtfgif-full.wasm
    
    echo "📊 WebAssembly module size:"
    ls -lh ../wtfgif-full.wasm
else
    echo "⚠️  wasm-opt not found, copying unoptimized binary..."
    cp target/wasm32-unknown-unknown/release/wtfgif.wasm ../wtfgif-full.wasm
    echo "💡 Install wasm-opt for better optimization: npm install -g wasm-opt"
fi

echo "✅ Build complete: wtfgif-full.wasm"
echo ""
echo "🎯 Module features:"
echo "  - Full LZW decoder in WebAssembly"
echo "  - SIMD-optimized palette mapping"
echo "  - Threading support (SharedArrayBuffer)"
echo "  - Memory management with bump allocator"
echo "  - Fallback-safe error handling"