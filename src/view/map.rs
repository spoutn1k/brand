use crate::{
    Aquiesce, Error, QueryExt,
    controller::{self, UIExposureUpdate, Update},
};
use leaflet::{LatLng, LatLngBounds, LayerGroup, Map, MapOptions, Marker, MouseEvent, TileLayer};
use std::cell::OnceCell;
use web_sys::HtmlElement;

thread_local! {
static MAP: OnceCell<Map> = const { OnceCell::new() };
static MARKERS: OnceCell<LayerGroup> = const { OnceCell::new() };
}

pub fn setup() -> Result<(), Error> {
    let map = Map::new_with_element(
        &"exposures-gps-map".query_id_into::<HtmlElement>()?,
        &MapOptions::default(),
    )?;

    TileLayer::new("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png").add_to(&map);
    map.on_mouse_click(Box::new(handle_click));

    // Generate the layer group managing the markers
    let layers = LayerGroup::new();
    layers.add_to(&map);

    MARKERS
        .with(|oc| oc.set(layers))
        .map_err(|_| Error::MapInit)?;

    MAP.try_with(|oc| oc.set(map))?
        .map_err(|_| Error::MapInit)?;

    reset();

    Ok(())
}

/// Reset map on Paris with a zoom of 4.0
pub fn reset() {
    MAP.with(|oc| {
        if let Some(map) = oc.get() {
            map.set_view(&LatLng::new(48.8566, 2.3522), 4.0);
        }
    })
}

pub fn invalidate() {
    MAP.with(|oc| {
        if let Some(m) = oc.get() {
            m.invalidate_size(true);
        }
    })
}

pub fn show_location(pos: &[(f64, f64)]) {
    // Get thread-local map
    MAP.with(|oc| {
        if let Some(m) = oc.get() {
            // Generate a Bounds object to store the positions
            let bounds = LatLngBounds::new_from_list(&js_sys::Array::new());

            // Generate or access the layer group managing the markers
            MARKERS.with(|oc| oc.get_or_init(LayerGroup::new).clear_layers());

            for (lat, lng) in pos.iter().cloned() {
                let location = LatLng::new(lat, lng);

                // Add the location to the bounds
                bounds.extend(&location);

                // Add a layer with the marker for this location to the layer group
                MARKERS.with(|oc| oc.get().map(|lg| lg.add_layer(&Marker::new(&location))));
            }

            // Display the markers using the bounds
            m.fly_to_bounds(&bounds);
        }
    })
}

fn handle_click(e: MouseEvent) {
    let position = e.lat_lng();
    let lat = position.lat();
    let lon = position.lng();

    controller::update(Update::Exposure(UIExposureUpdate::GpsMap(lat, lon))).aquiesce();
}
