import init from './brand.js'

async function init_wasm_in_worker() {
    self.onmessage = async (e) => {
        const wasm = await init({
            module_or_path: '/brand_bg.wasm'
        });

        self.postMessage(await wasm.handle_message(e.data));
    };
};

init_wasm_in_worker()
