use rltk::prelude::*;
use specs::prelude::*;
use specs::saveload::{SimpleMarker, SimpleMarkerAllocator};

//Macros
macro_rules! register_all {
    ($world:expr, $($component:ty),* $(,)*) => {
        {
            $($world.register::<$component>();)*
        }
    };
}

//All mods
mod components;
mod damage_system;
mod gamelog;
mod gui;
mod item_systems;
mod map;
mod map_indexing_system;
mod melee_combat_system;
mod monster_ai_system;
mod player;
mod random_table;
mod rect;
mod saveload_system;
mod spawner;
mod visibility_system;

use components::*;
use damage_system::*;
use gamelog::*;
use item_systems::*;
use map::*;
use map_indexing_system::*;
use melee_combat_system::*;
use monster_ai_system::*;
use player::*;
use random_table::*;
use spawner::*;
use visibility_system::*;

//Enums
#[derive(PartialEq, Copy, Clone, Debug)]
pub enum RunState {
    AwaitingInput,
    GameOver,
    MainMenu(gui::MainMenuSelection),
    MonsterTurn,
    NextLevel,
    PlayerTurn,
    PreRun,
    SaveGame,
    ShowDropItem,
    ShowInventory,
    ShowRemoveItem,
    ShowTargeting(i32, Entity),
}

pub struct State {
    pub ecs: World,
}

impl State {
    fn run_systems(&mut self) {
        let mut mons = MonsterAI {};
        let mut damage = DamageSystem {};
        let mut vis = VisibilitySystem {};
        let mut melee = MeleeCombatSystem {};
        let mut mapindex = MapIndexingSystem {};
        let mut pickup_items = ItemCollectionSystem {};
        let mut drop_items = ItemDropSystem {};
        let mut use_items = ItemUseSystem {};
        let mut rem_items = ItemRemoveSystem {};

        vis.run_now(&self.ecs);
        mons.run_now(&self.ecs);
        mapindex.run_now(&self.ecs);
        melee.run_now(&self.ecs);
        damage.run_now(&self.ecs);
        pickup_items.run_now(&self.ecs);
        drop_items.run_now(&self.ecs);
        use_items.run_now(&self.ecs);
        rem_items.run_now(&self.ecs);

        self.ecs.maintain();
    }

    fn entities_to_remove_on_level_change(&mut self) -> Vec<Entity> {
        let entities = self.ecs.entities();
        let player_ent = self.ecs.fetch::<Entity>();
        let backpack = self.ecs.read_storage::<InBackpack>();
        let equipped_items = self.ecs.read_storage::<Equipped>();

        let mut to_delete = entities.join().collect::<Vec<_>>();
        to_delete.retain(|ent| {
            let is_player = *ent == *player_ent;
            let is_in_player_bag = {
                if let Some(pack) = backpack.get(*ent) {
                    pack.owner == *player_ent
                } else {
                    false
                }
            };
            let is_equipped_by_player = {
                if let Some(eq) = equipped_items.get(*ent) {
                    eq.owner == *player_ent
                } else {
                    false
                }
            };
            !is_player && !is_in_player_bag && !is_equipped_by_player
        });

        to_delete
    }

    fn goto_next_level(&mut self) {
        let to_delete = self.entities_to_remove_on_level_change();
        for target in to_delete {
            self.ecs
                .delete_entity(target)
                .expect("Unable to delete entity during level transition");
        }

        let world_map = {
            let mut world_map = self.ecs.write_resource::<Map>();
            let current_depth = world_map.depth;
            *world_map = Map::new_map_rooms_and_corridors(current_depth + 1);
            world_map.clone()
        };

        //Baddies
        for room in world_map.rooms.iter().skip(1) {
            spawner::populate_room(&mut self.ecs, room, world_map.depth);
        }

        //Update Player pos, and Player comp resource
        let (player_x, player_y) = world_map.rooms[0].center();
        let mut player_pos = self.ecs.write_resource::<Point>();
        *player_pos = Point::new(player_x, player_y);
        let mut pos_comps = self.ecs.write_storage::<Position>();
        let player_ent = self.ecs.fetch::<Entity>();
        if let Some(pos_comp) = pos_comps.get_mut(*player_ent) {
            pos_comp.x = player_x;
            pos_comp.y = player_y;
        }

        //Dirty players viewshed
        let mut viewsheds = self.ecs.write_storage::<Viewshed>();
        if let Some(vs) = viewsheds.get_mut(*player_ent) {
            vs.is_dirty = true;
        }

        //Notify player
        let mut logs = self.ecs.fetch_mut::<gamelog::GameLog>();
        logs.entries
            .push("You descend to the next level.".to_string());
        let mut all_stats = self.ecs.write_storage::<CombatStats>();
        if let Some(player_stats) = all_stats.get_mut(*player_ent) {
            player_stats.hp = i32::max(player_stats.hp, player_stats.max_hp / 2);
        }
    }

    fn game_over_cleanup(&mut self) {
        let to_delete = self.ecs.entities().join().collect::<Vec<_>>();
        for ent in to_delete.iter() {
            self.ecs.delete_entity(*ent).expect("Deletion failed");
        }
        let world_map = {
            let mut world_map = self.ecs.write_resource::<Map>();
            *world_map = Map::new_map_rooms_and_corridors(1);
            world_map.clone()
        };

        //Baddies
        for room in world_map.rooms.iter().skip(1) {
            spawner::populate_room(&mut self.ecs, room, world_map.depth);
        }

        //Restart World
        let (player_x, player_y) = world_map.rooms[0].center();
        let new_player_ent = spawner::spawn_player(&mut self.ecs, player_x, player_y);

        //Update player resources
        self.ecs.insert(new_player_ent);
        self.ecs.insert(Point::new(player_x, player_y));

        //Update player movement comp
        let mut position_components = self.ecs.write_storage::<Position>();
        if let Some(player_pos_comp) = position_components.get_mut(new_player_ent) {
            player_pos_comp.x = player_x;
            player_pos_comp.y = player_y;
        }

        //Dirty players viewshed
        let mut viewsheds = self.ecs.write_storage::<Viewshed>();
        if let Some(vs) = viewsheds.get_mut(new_player_ent) {
            vs.is_dirty = true;
        }
    }
}

impl GameState for State {
    fn tick(&mut self, ctx: &mut Rltk) {
        ctx.cls();

        let mut next_state = *self.ecs.fetch::<RunState>();
        //Draw map & entities
        match next_state {
            RunState::MainMenu(_) => {}
            _ => {
                draw_map(&self.ecs, ctx);
                {
                    let positions = self.ecs.read_storage::<Position>();
                    let renderables = self.ecs.read_storage::<Renderable>();
                    let map = self.ecs.fetch::<Map>();
                    let mut data = (&positions, &renderables).join().collect::<Vec<_>>();
                    data.sort_by(|&a, &b| b.1.render_order.cmp(&a.1.render_order));
                    for (pos, render) in data.iter() {
                        let idx = map.xy_idx(pos.x, pos.y);
                        if map.is_tile_status_set(idx, TILE_VISIBLE) {
                            ctx.set(pos.x, pos.y, render.fg, render.bg, render.glyph);
                        }
                    }
                }

                //GUI
                gui::draw_ui(&self.ecs, ctx);
            }
        }

        //FSM
        match next_state {
            RunState::PreRun => {
                self.run_systems();
                next_state = RunState::AwaitingInput;
            }
            RunState::AwaitingInput => {
                next_state = player_input(self, ctx);
            }
            RunState::PlayerTurn => {
                self.run_systems();
                next_state = RunState::MonsterTurn;
            }
            RunState::MonsterTurn => {
                self.run_systems();
                next_state = RunState::AwaitingInput;
            }
            RunState::ShowInventory => {
                let (item_res, selected_item) = gui::show_inventory(self, ctx);
                match item_res {
                    gui::ItemMenuResult::Selected => {
                        let selected_item = selected_item.unwrap();
                        if let Some(range) = self.ecs.read_storage::<Ranged>().get(selected_item) {
                            next_state = RunState::ShowTargeting(range.range, selected_item);
                        } else {
                            let mut intent = self.ecs.write_storage::<WantsToUseItem>();
                            intent
                                .insert(
                                    *self.ecs.fetch::<Entity>(),
                                    WantsToUseItem {
                                        item: selected_item,
                                        target: None,
                                    },
                                )
                                .expect("Unable to insert intent");
                            next_state = RunState::PlayerTurn;
                        }
                    }
                    gui::ItemMenuResult::Cancel => next_state = RunState::AwaitingInput,
                    gui::ItemMenuResult::NoResponse => {}
                }
            }
            RunState::ShowDropItem => {
                let (item_res, selected_item) = gui::show_inventory(self, ctx);
                match item_res {
                    gui::ItemMenuResult::Selected => {
                        let selected_item = selected_item.unwrap();
                        let mut intent = self.ecs.write_storage::<WantsToDropItem>();
                        intent
                            .insert(
                                *self.ecs.fetch::<Entity>(),
                                WantsToDropItem {
                                    item: selected_item,
                                },
                            )
                            .expect("Unable to insert intent to drop item");
                        next_state = RunState::PlayerTurn;
                    }
                    gui::ItemMenuResult::Cancel => next_state = RunState::AwaitingInput,
                    gui::ItemMenuResult::NoResponse => {}
                }
            }
            RunState::ShowRemoveItem => {
                let (item_res, selected_item) = gui::show_remove_inventory(self, ctx);
                match item_res {
                    gui::ItemMenuResult::Selected => {
                        let selected_item = selected_item.unwrap();
                        let mut intent = self.ecs.write_storage::<WantsToRemoveItem>();
                        intent
                            .insert(
                                *self.ecs.fetch::<Entity>(),
                                WantsToRemoveItem {
                                    item: selected_item,
                                },
                            )
                            .expect("Unable to insert intent to remove item");
                        next_state = RunState::PlayerTurn;
                    }
                    gui::ItemMenuResult::Cancel => next_state = RunState::AwaitingInput,
                    gui::ItemMenuResult::NoResponse => {}
                }
            }
            RunState::ShowTargeting(range, item) => {
                let (item_res, target) = gui::draw_range(self, ctx, range);
                match item_res {
                    gui::ItemMenuResult::Selected => {
                        let mut intent = self.ecs.write_storage::<WantsToUseItem>();
                        intent
                            .insert(*self.ecs.fetch::<Entity>(), WantsToUseItem { item, target })
                            .expect("Unable to insert intent");
                        next_state = RunState::PlayerTurn;
                    }
                    gui::ItemMenuResult::Cancel => next_state = RunState::AwaitingInput,
                    gui::ItemMenuResult::NoResponse => {}
                }
            }
            RunState::MainMenu(_) => match gui::draw_main_menu(self, ctx) {
                gui::MainMenuResult::NoSelection(prev_option) => {
                    next_state = RunState::MainMenu(prev_option)
                }
                gui::MainMenuResult::Selection(option) => match option {
                    gui::MainMenuSelection::NewGame => next_state = RunState::PreRun,
                    gui::MainMenuSelection::LoadGame => {
                        if saveload_system::does_save_exist() {
                            saveload_system::load_game(&mut self.ecs);
                            next_state = RunState::AwaitingInput;
                            saveload_system::delete_save();
                        } else {
                            next_state = RunState::MainMenu(option);
                        }
                    }
                    gui::MainMenuSelection::Quit => std::process::exit(0),
                },
            },
            RunState::GameOver => {
                let result = gui::show_game_over(ctx);
                match result {
                    gui::GameOverResult::NoSelection => {}
                    gui::GameOverResult::QuitToMenu => {
                        self.game_over_cleanup();
                        next_state = RunState::MainMenu(gui::MainMenuSelection::NewGame);
                    }
                }
            }
            RunState::SaveGame => {
                saveload_system::save_game(&mut self.ecs);
                next_state = RunState::MainMenu(gui::MainMenuSelection::LoadGame);
            }
            RunState::NextLevel => {
                self.goto_next_level();
                next_state = RunState::PreRun;
            }
        }

        //Replace RunState with the new one
        self.ecs.insert::<RunState>(next_state);
        DamageSystem::delete_the_dead(&mut self.ecs);
    }
}

rltk::embedded_resource!(GAME_DEFAULT, "../resources/cp437_16x16.png");

fn main() -> BError {
    rltk::link_resource!(GAME_DEFAULT, "resources/cp437_16x16.png");
    let context = {
        let mut context = RltkBuilder::simple80x50()
            .with_title("Bashing Bytes")
            //.with_font("cp437_16x16.png", 16, 16)
            .build()?;
        //context.set_active_font(1, true); //Eventually after decoupling map from screen size
        //after decoupling map from screen size
        context
    };

    //Construct world
    let mut gs = State { ecs: World::new() };

    //Register the components
    register_all!(
        gs.ecs,
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
        SimpleMarker<SerializeMe>,
        SufferDamage,
        Viewshed,
        WantsToDropItem,
        WantsToMelee,
        WantsToPickupItem,
        WantsToRemoveItem,
        WantsToUseItem,
    );

    //Inserts that must be implemented before creating entities
    gs.ecs.insert(SimpleMarkerAllocator::<SerializeMe>::new());
    gs.ecs.insert(rltk::RandomNumberGenerator::new());

    //Create map and get player location
    let map = Map::new_map_rooms_and_corridors(1);

    //Populate rooms in the map
    for room in map.rooms.iter().skip(1) {
        populate_room(&mut gs.ecs, &room, map.depth);
    }
    let (player_x, player_y) = map.rooms[0].center();
    let player_ent = spawn_player(&mut gs.ecs, player_x, player_y);

    //Final resources to insert into world
    gs.ecs.insert(map);
    gs.ecs.insert(player_ent);
    gs.ecs.insert(Point::new(player_x, player_y));
    gs.ecs
        .insert(RunState::MainMenu(gui::MainMenuSelection::NewGame));
    gs.ecs.insert(GameLog {
        entries: vec!["Welcome to my roguelike".to_string()],
    });

    //Start game
    main_loop(context, gs)
}
