//! Geographic preparation for isotrail: local projection, the terrain grid,
//! elevation and map data sources, the on-disk cache that keeps them
//! offline-friendly and assembly of the scene bundle
//!
//! ```
//! use isotrail_geo::{DEFAULT_DETAIL, Footprint, Grid};
//! use isotrail_track::{Segment, Track, TrackPoint};
//!
//! let at = |lat, lon| TrackPoint { lat, lon, ele: None, time: None };
//! let track = Track {
//!     segments: vec![Segment { points: vec![at(46.50, 10.0), at(46.59, 10.0)] }],
//!     ..Track::default()
//! };
//!
//! let footprint = Footprint::from_track(&track)?;
//! assert_eq!(footprint.long_axis_bearing_deg().round(), 0.0);
//! let grid = Grid::new(&footprint, DEFAULT_DETAIL);
//! assert_eq!((grid.cols, grid.rows), (256, 128));
//! # Ok::<(), isotrail_geo::GeoError>(())
//! ```

mod dem;
mod error;
mod fetch;
mod footprint;
mod grid;
mod landcover;
mod osm;
mod projection;
mod relief;
mod scene;

pub use dem::{
    DemError, DemSource, Heightmap, MAX_MERCATOR_LAT, TERRARIUM_URL, TILE_SIZE, TerrariumSource,
    TileId, lat_lon_to_pixel, pixel_to_lat_lon,
};
pub use error::GeoError;
pub use fetch::{
    Cache, CacheStats, CachedFetcher, FetchError, Fetcher, HttpFetcher, RecordedFetcher,
    RetryPolicy, USER_AGENT,
};
pub use footprint::{Footprint, MAX_ASPECT, MIN_PADDING_M, PADDING_FRACTION};
pub use grid::{DEFAULT_DETAIL, Grid, MAX_DEM_ZOOM, dem_zoom, ground_resolution_m};
pub use isotrail_bundle::{Landcover, Provenance, SceneBundle, line_mask};
pub use landcover::{
    ALPINE_ROCK_SLOPE_DEG, LandcoverMap, ROCK_ABOVE_TREELINE_M, ROCK_SLOPE_DEG, Treeline,
    WATER_LEVEL_PERCENTILE, WaterBody, latitude_treeline_m, slope_deg,
};
pub use osm::{
    Area, BoundingBox, FEATURE_KEYS, FeatureSource, Line, MINOR_WAY_MAX_CELL_M, OVERPASS_URL,
    OsmError, OsmFeatures, OsmId, OsmKind, OverpassSource, Polygon, Ring, Tags, overpass_query,
};
pub use projection::LocalProjection;
pub use relief::{EXAGGERATION_FACTOR, MAX_EXAGGERATION, MIN_EXAGGERATION, vertical_exaggeration};
pub use scene::{
    DEM_ATTRIBUTION, OSM_ATTRIBUTION, PROFILE_SAMPLES, ROUTE_BREAK_M, ROUTE_TOLERANCE_CELLS,
    scene_bundle,
};
