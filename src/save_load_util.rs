use crate::{components::*, Map};
use specs::error::NoError;
use specs::prelude::*;
use specs::saveload::{
    DeserializeComponents, MarkedBuilder, SerializeComponents, SimpleMarker, SimpleMarkerAllocator,
};
use std::fs;
use std::path::Path;

macro_rules! serialize_individually {
    ($ecs:expr, $ser:expr, $data:expr, $( $type:ty),* $(,)?) => {
        $(
        SerializeComponents::<NoError, SimpleMarker<SerializeMe>>::serialize(
            &( $ecs.read_storage::<$type>(), ),
            &$data.0,
            &$data.1,
            &mut $ser,
        )
        .unwrap();
        )*
    };
}

macro_rules! deserialize_individually {
    ($ecs:expr, $de:expr, $data:expr, $( $type:ty),* $(,)?) => {
        $(
        DeserializeComponents::<NoError, _>::deserialize(
            &mut ( &mut $ecs.write_storage::<$type>(), ),
            &$data.0, // entities
            &mut $data.1, // marker
            &mut $data.2, // allocater
            &mut $de,
        )
        .unwrap();
        )*
    };
}

pub fn save_game(ecs: &mut World) {
    let mapcopy = ecs.get_mut::<Map>().unwrap().clone();
    let save_helper = ecs
        .create_entity()
        .with(SerializationHelper { map: mapcopy })
        .marked::<SimpleMarker<SerializeMe>>()
        .build();
    {
        let data = (
            ecs.entities(),
            ecs.read_storage::<SimpleMarker<SerializeMe>>(),
        );
        let writer = std::fs::File::create("./saves/savegame.json").unwrap();
        let mut serializer = serde_json::Serializer::new(writer);
        serialize_individually!(
            ecs,
            serializer,
            data,
            AreaOfEffect,
            BlocksTile,
            CombatStats,
            Consumable,
            DefenseBonus,
            Equipable,
            Equipped,
            InBackpack,
            InflictsDamage,
            Item,
            MeleeDamageBonus,
            Monster,
            Name,
            Player,
            Position,
            ProvidesHealing,
            Ranged,
            Renderable,
            SerializationHelper,
            SufferDamage,
            Viewshed,
            WantsToDropItem,
            WantsToMelee,
            WantsToPickupItem,
            WantsToRemoveItem,
            WantsToUseItem,
        );
    }

    ecs.delete_entity(save_helper)
        .expect("Unable to delete save helper");
}

pub fn load_game(ecs: &mut World) {
    {
        let mut to_delete = Vec::new();
        for e in ecs.entities().join() {
            to_delete.push(e);
        }
        for del in to_delete.iter() {
            ecs.delete_entity(*del).expect("Deletion failed");
        }
    }

    let data = fs::read_to_string("./savegame.json").unwrap();
    let mut de = serde_json::Deserializer::from_str(&data);

    {
        let mut d = (
            &mut ecs.entities(),
            &mut ecs.write_storage::<SimpleMarker<SerializeMe>>(),
            &mut ecs.write_resource::<SimpleMarkerAllocator<SerializeMe>>(),
        );
        deserialize_individually!(
            ecs,
            de,
            d,
            AreaOfEffect,
            BlocksTile,
            CombatStats,
            Consumable,
            Equipable,
            Equipped,
            InBackpack,
            InflictsDamage,
            Item,
            Monster,
            Name,
            Player,
            Position,
            ProvidesHealing,
            Ranged,
            Renderable,
            SerializationHelper,
            SufferDamage,
            Viewshed,
            WantsToDropItem,
            WantsToMelee,
            WantsToPickupItem,
            WantsToRemoveItem,
            WantsToUseItem,
        );
    }

    let mut delete_me = None;
    {
        let entities = ecs.entities();
        let helper = ecs.read_storage::<SerializationHelper>();
        let player = ecs.read_storage::<Player>();
        let position = ecs.read_storage::<Position>();
        for (e, h) in (&entities, &helper).join() {
            let mut world_map = ecs.write_resource::<Map>();
            *world_map = h.map.clone();
            world_map.tile_content =
                vec![Vec::new(); (world_map.width * world_map.height) as usize];
            delete_me = Some(e);
        }
        for (e, _, pos) in (&entities, &player, &position).join() {
            let mut player_pos = ecs.write_resource::<rltk::Point>();
            let mut player_ent = ecs.write_resource::<Entity>();
            *player_pos = rltk::Point::new(pos.x, pos.y);
            *player_ent = e;
        }
    }

    ecs.delete_entity(delete_me.unwrap())
        .expect("Unable to delete helper");
}

pub fn does_save_exist() -> bool {
    Path::new("./savegame.json").exists()
}

pub fn delete_save() {
    if does_save_exist() {
        std::fs::remove_file("./savegame.json").expect("Unable to delete file");
    }
}
