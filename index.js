var map;
var marker;

var filehandles = new Map();

document.addEventListener('DOMContentLoaded', (event) => {
    document.getElementById('photoselect').value = ""

});

document.addEventListener('DOMContentLoaded', (event) => {
    map = L.map('map').setView([48.8566, 2.3522], 3);

    L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
        attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors'
    }).addTo(map);
});

function prompt_coords(index) {
    document.getElementById("map").classList.toggle("hidden");

    setTimeout(function() {
        map.invalidateSize()
    }, 200);

    function update_exposure(e) {
        const {
            lat,
            lng
        } = e.latlng;

        window.wasmBindings.update_coords(index, lat.toFixed(8), lng.toFixed(8));
        if (marker) {
            map.removeLayer(marker);
        }

        document.getElementById("map").classList.toggle("hidden");

        map.off('click', update_exposure);
    }

    map.on('click', update_exposure);
}

function set_marker(lat, lng) {
    if (marker) {
        map.removeLayer(marker);
    }

    marker = L.marker([lat, lng]).addTo(map);
}

function get_raw_handles() {
    return filehandles
}

function set_raw_handles(value) {
    filehandles = value
}
