import init from './brand.js'

async function init_wasm_in_worker() {
    self.onmessage = async (e) => {
        // Load the wasm file by awaiting the Promise returned by `wasm_bindgen`.
        const wasm = await init({ module_or_path: '/brand_bg.wasm' });
        await wasm.handle_message(e.data)

        self.onmessage = wasm.handle_message
    };
};

init_wasm_in_worker()
