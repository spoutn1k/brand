use crate::{
    Aquiesce, Error, QueryExt,
    controller::{self, UIExposureUpdate, Update},
};
use leaflet::{LatLng, LayerGroup, Map, MapOptions, Marker, MouseEvent, TileLayer};
use std::cell::OnceCell;
use web_sys::HtmlElement;

thread_local! {
    static MAP: OnceCell<Map> = OnceCell::new();
    static MARKERS: OnceCell<LayerGroup> = OnceCell::new();
}

pub fn setup() -> Result<(), Error> {
    let map = Map::new_with_element(
        &"exposures-gps-map".query_id_into::<HtmlElement>()?,
        &MapOptions::default(),
    );

    map.set_view(&LatLng::new(48.8566, 2.3522), 3.0);
    TileLayer::new("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png").add_to(&map);
    map.on_mouse_click(Box::new(handle_click));

    MAP.try_with(|oc| oc.set(map))?
        .map_err(|_| Error::MapInit)?;

    Ok(())
}

pub fn invalidate() {
    MAP.with(|oc| {
        oc.get().map(|m| {
            m.invalidate_size(true);
        });
    })
}

pub fn show_location(lat: f64, lon: f64) {
    MAP.with(|oc| {
        oc.get().map(|m| {
            MARKERS.with(|oc| oc.get_or_init(LayerGroup::new).clear_layers());

            {
                let location = LatLng::new(lat, lon);
                let marker = Marker::new(&location);

                MARKERS.with(|oc| oc.get().map(|lg| lg.add_layer(&marker)));

                m.pan_to(&location);
            }

            MARKERS.with(|oc| oc.get().map(|lg| lg.add_to(&m)));

            m.set_zoom(8.0);
        });
    })
}

fn handle_click(e: MouseEvent) {
    let position = e.lat_lng();
    let lat = position.lat();
    let lon = position.lng();

    controller::update(Update::Exposure(UIExposureUpdate::Gps(format!(
        "{lat}, {lon}"
    ))))
    .aquiesce();
}
