enum Adapter {
    None,
    File { path: String },
    Gcs { bucket: String },
}

struct FallbackRouteMap {
    r#type: Adapter,
}

struct Config {
    fallback_route_map: Option<FallbackRouteMap>
}