document.addEventListener('DOMContentLoaded', (event) => {
    document.getElementById('photoselect').value = ""
});

addEventListener("TrunkApplicationStarted", (e) => window.wasmBindings.setup());
