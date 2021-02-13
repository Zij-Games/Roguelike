use super::{Map, Rect, TileType};
use std::collections::HashMap;

const MAX_STEPS: f32 = 200.0;

///Given a room, it fills the inner part of the with floors.
pub fn apply_room_to_map(map: &mut Map, room: &Rect) {
    for y in room.y1 + 1..room.y2 {
        for x in room.x1 + 1..room.x2 {
            let idx = map.xy_idx(x, y);
            map.tiles[idx] = TileType::Floor;
        }
    }
}

pub fn apply_horizontal_tunnel(map: &mut Map, x1: i32, x2: i32, y: i32) {
    for x in std::cmp::min(x1, x2)..=std::cmp::max(x1, x2) {
        let idx = map.xy_idx(x, y);
        map.tiles[idx as usize] = TileType::Floor;
    }
}

pub fn apply_vertical_tunnel(map: &mut Map, y1: i32, y2: i32, x: i32) {
    for y in std::cmp::min(y1, y2)..=std::cmp::max(y1, y2) {
        let idx = map.xy_idx(x, y);
        map.tiles[idx as usize] = TileType::Floor;
    }
}

pub fn gen_voronoi_regions(
    map: &Map,
    rng: &mut rltk::RandomNumberGenerator,
) -> HashMap<i32, Vec<(i32, i32)>> {
    let mut noise_areas: HashMap<i32, Vec<(i32, i32)>> = HashMap::new();
    let mut noise = rltk::FastNoise::seeded(rng.roll_dice(1, 65536) as u64);
    noise.set_noise_type(rltk::NoiseType::Cellular);
    noise.set_frequency(0.08);
    noise.set_cellular_distance_function(rltk::CellularDistanceFunction::Manhattan);

    for y in 1..map.height - 1 {
        for x in 1..map.width - 1 {
            let idx = map.xy_idx(x, y);
            if map.tiles[idx] == TileType::Floor {
                let cell_value_f = noise.get_noise(x as f32, y as f32) * 10240.0;
                let cell_value_i = cell_value_f as i32;
                noise_areas
                    .entry(cell_value_i)
                    .or_insert(vec![])
                    .push((x, y));
            }
        }
    }
    noise_areas
}

pub fn cull_and_set_exit(map: &mut Map, start_idx: usize) {
    let dijkstra_map = rltk::DijkstraMap::new(
        map.width,
        map.height,
        &[start_idx],
        &*map,
        MAX_STEPS, //Stop counting at max steps
    );
    let mut exit_tile = (0, 0.0);

    for (i, tile) in map.tiles.iter_mut().enumerate() {
        if *tile == TileType::Floor {
            let distance_to_start = dijkstra_map.map[i];
            if distance_to_start == f32::MAX {
                //If unreachable, make it a wall
                *tile = TileType::Wall;
            } else {
                //Put exit furthest point away from player
                if distance_to_start > exit_tile.1 {
                    exit_tile.0 = i;
                    exit_tile.1 = distance_to_start;
                }
            }
        }
    }
    map.tiles[exit_tile.0] = TileType::StairsDown;
}
