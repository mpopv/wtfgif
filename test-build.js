// test-build.js - Test that the compiled module works
try {
    console.log('🔧 Testing compiled wtfgif module...');
    
    const wtfgif = require('./wtfgif.js');
    
    console.log('✅ Module loads successfully');
    console.log('Available exports:', Object.keys(wtfgif));
    console.log('GifReader available:', !!wtfgif.GifReader);
    console.log('GifWriter available:', !!wtfgif.GifWriter);
    console.log('initializeWasmGlobally available:', !!wtfgif.initializeWasmGlobally);
    console.log('getWasmStatus available:', !!wtfgif.getWasmStatus);
    
    // Test Wasm status
    if (wtfgif.getWasmStatus) {
        const wasmStatus = wtfgif.getWasmStatus();
        console.log('WebAssembly features:', wasmStatus);
    }
    
    console.log('\n🎯 Build test: SUCCESS');
    
} catch (error) {
    console.error('❌ Build test failed:', error.message);
    process.exit(1);
}