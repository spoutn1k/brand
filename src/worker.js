import init from './brand.js'

self.onmessage = async (e) => {
    const wasm = await init({
        module_or_path: '/brand_bg.wasm'
    });

    self.postMessage(await wasm.handle_message(e.data));
};
