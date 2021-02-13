//External includes
use rltk::prelude::*;
use specs::prelude::*;
use specs::saveload::{SimpleMarker, SimpleMarkerAllocator};

//Internal mods and includes
mod ecs;
mod gui;
mod map_builder;
mod player;
mod random_table;
mod rex_assets;
mod save_load_util;
mod spawner;

use ecs::*;
use map_builder::*;
use player::*;
use random_table::*;

//Constants
const SHOW_MAPGEN: bool = false;

//Macros
///Given a specs::World, and a list of components, it registers all components in the world
macro_rules! register_all {
    ($ecs:expr, $($component:ty),* $(,)*) => {
        {
            $($ecs.register::<$component>();)*
        }
    };
}

///Given a specs::World, and a list of resources, it inserts all resources in the world
macro_rules! insert_all {
    ($ecs:expr, $($resource:expr),* $(,)*) => {
        {
            $($ecs.insert($resource);)*
        }
    };
}

#[derive(PartialEq, Copy, Clone, Debug)]
pub enum RunState {
    AwaitingInput,
    GameOver,
    MainMenu(gui::MainMenuSelection),
    MapGeneration,
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

//Main gamestate
pub struct State {
    pub ecs: World,
    mapgen_next_state: Option<RunState>,
    mapgen_history: Vec<Map>,
    mapgen_index: usize,
    mapgen_timer: f32,
}

impl State {
    ///Runs through all systems
    fn run_systems(&mut self) {
        let mut vis = VisibilitySystem {};
        let mut mons = MonsterAI {};
        let mut mapindex = MapIndexingSystem {};
        let mut melee = MeleeCombatSystem {};
        let mut damage = DamageSystem {};
        let mut pickup_items = ItemCollectionSystem {};
        let mut use_items = ItemUseSystem {};
        let mut drop_items = ItemDropSystem {};
        let mut rem_items = ItemRemoveSystem {};
        let mut particles = ParticleSpawnSystem {};

        vis.run_now(&self.ecs);
        mons.run_now(&self.ecs);
        mapindex.run_now(&self.ecs);
        melee.run_now(&self.ecs);
        damage.run_now(&self.ecs);
        pickup_items.run_now(&self.ecs);
        use_items.run_now(&self.ecs);
        drop_items.run_now(&self.ecs);
        rem_items.run_now(&self.ecs);
        particles.run_now(&self.ecs);

        self.ecs.maintain();
    }

    ///Gathers all entities that are not related to the player
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

    ///Generates next level for the player to explore
    fn goto_next_level(&mut self) {
        let to_delete = self.entities_to_remove_on_level_change();
        for target in to_delete {
            self.ecs
                .delete_entity(target)
                .expect("Unable to delete entity during level transition");
        }

        //Build new map and place player
        let current_depth = self.ecs.fetch::<Map>().depth;
        self.generate_world_map(current_depth + 1);

        //Notify player and heal player
        let player_ent = self.ecs.fetch::<Entity>();
        let mut logs = self.ecs.fetch_mut::<GameLog>();
        logs.entries
            .push("You descend to the next level.".to_string());
        let mut all_stats = self.ecs.write_storage::<CombatStats>();
        if let Some(player_stats) = all_stats.get_mut(*player_ent) {
            player_stats.hp = i32::max(player_stats.hp, player_stats.max_hp / 2);
        }
    }

    ///Deletes all entities, and sets up for next game
    fn game_over_cleanup(&mut self) {
        self.ecs.delete_all();
        self.ecs.maintain();

        //Add starting message
        let mut logs = self.ecs.write_resource::<GameLog>();
        logs.entries.clear();
        logs.entries.push("Welcome to my Roguelike!".to_string());
        std::mem::drop(logs);

        //Create new player resource
        let player_ent = spawner::spawn_player(&mut self.ecs, 0, 0);
        self.ecs.insert(player_ent);
        self.ecs.insert(Point::new(0, 0));

        //Build a new map and place player
        self.generate_world_map(1);
    }

    ///Generates a new level using random_builder with the specified depth
    fn generate_world_map(&mut self, new_depth: i32) {
        //Visualizing mapgen
        self.mapgen_index = 0;
        self.mapgen_timer = 0.0;
        self.mapgen_history.clear();

        let mut builder = map_builder::random_builder(new_depth);
        builder.build_map();
        {
            let mut world = self.ecs.write_resource::<Map>();
            *world = builder.get_map();
        }
        self.mapgen_history = builder.get_snapshot_history();

        builder.spawn_entities(&mut self.ecs);

        //Updates the players position based on the new map generated
        //Also must update the player component, and the player pos resource
        let player_start = builder.get_starting_position();
        let (player_x, player_y) = (player_start.x, player_start.y);
        let mut player_position = self.ecs.write_resource::<Point>();
        *player_position = Point::new(player_x, player_y);
        let mut position_components = self.ecs.write_storage::<Position>();
        let player_ent = self.ecs.fetch::<Entity>();
        if let Some(player_pos_comp) = position_components.get_mut(*player_ent) {
            player_pos_comp.x = player_x;
            player_pos_comp.y = player_y;
        }

        let mut viewsheds = self.ecs.write_storage::<Viewshed>();
        if let Some(vs) = viewsheds.get_mut(*player_ent) {
            vs.is_dirty = true;
        }
    }
}

impl GameState for State {
    fn tick(&mut self, ctx: &mut Rltk) {
        ctx.cls();
        particle_system::cull_dead_particles(&mut self.ecs, ctx);

        let mut next_state = *self.ecs.fetch::<RunState>();

        //Draw map & renderables
        match next_state {
            RunState::MainMenu(_) => {}
            _ => {
                draw_map(&self.ecs.fetch::<Map>(), ctx);
                {
                    let positions = self.ecs.read_storage::<Position>();
                    let renderables = self.ecs.read_storage::<Renderable>();
                    let map = self.ecs.fetch::<Map>();
                    let mut data = (&positions, &renderables).join().collect::<Vec<_>>();
                    data.sort_by(|&a, &b| b.1.render_order.cmp(&a.1.render_order));
                    for (pos, render) in data.iter() {
                        let idx = map.xy_idx(pos.x, pos.y);
                        if map.is_tile_status_set(idx, TileStatus::Visible) {
                            ctx.set(pos.x, pos.y, render.fg, render.bg, render.glyph);
                        }
                    }
                }

                //GUI
                gui::draw_ui(&self.ecs, ctx);
            }
        }

        //Calculates next state based on current state
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
            RunState::SaveGame => {
                save_load_util::save_game(&mut self.ecs);
                next_state = RunState::MainMenu(gui::MainMenuSelection::LoadGame);
            }
            RunState::NextLevel => {
                self.goto_next_level();
                next_state = RunState::PreRun;
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
                    gui::MainMenuSelection::NewGame => {
                        self.game_over_cleanup();
                        next_state = RunState::PreRun;
                        if SHOW_MAPGEN {
                            next_state = RunState::MapGeneration;
                        }
                    }
                    gui::MainMenuSelection::LoadGame => {
                        if save_load_util::does_save_exist() {
                            save_load_util::load_game(&mut self.ecs);
                            next_state = RunState::AwaitingInput;
                            save_load_util::delete_save();
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
            RunState::MapGeneration => {
                if !SHOW_MAPGEN {
                    next_state = self.mapgen_next_state.unwrap();
                } else {
                    ctx.cls();
                    draw_map(&self.mapgen_history[self.mapgen_index], ctx);

                    self.mapgen_timer += ctx.frame_time_ms;
                    if self.mapgen_timer > 200.0 {
                        self.mapgen_timer = 0.0;
                        self.mapgen_index += 1;
                        if self.mapgen_index >= self.mapgen_history.len() {
                            next_state = self.mapgen_next_state.unwrap();
                        }
                    }
                }
            }
        }

        //Replace RunState with the new one
        self.ecs.insert::<RunState>(next_state);
        DamageSystem::delete_the_dead(&mut self.ecs);
    }
}

pub struct GameLog {
    pub entries: Vec<String>,
}

fn main() -> BError {
    let context = RltkBuilder::simple(80, 60)
        .unwrap()
        .with_title("Bashing Bytes")
        .with_fullscreen(true)
        .build()?;

    //Construct world
    let mut gs = State {
        ecs: World::new(),
        mapgen_next_state: Some(RunState::MainMenu(gui::MainMenuSelection::NewGame)),
        mapgen_history: Vec::new(),
        mapgen_index: 0,
        mapgen_timer: 0.0,
    };

    //Register the components
    //gs.ecs must be first, otherwise irrelevant
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
        ParticleLifetime,
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

    //gs.ecs must be first, otherwise follow the dependencies
    //DEPENDANCIES:
    //player -> SimpleMarkerAllocator
    insert_all!(
        gs.ecs,
        SimpleMarkerAllocator::<SerializeMe>::new(),
        rltk::RandomNumberGenerator::new(),
        Map::new(1),
        Point::new(0, 0),
        RunState::MapGeneration {},
        particle_system::ParticleBuilder::new(),
        rex_assets::RexAssets::new(),
        GameLog {
            entries: vec!["Welcome to my roguelike".to_string()],
        },
    );

    //Unable to include this statement in the above batch due to the borrow checker
    //Reason: Both World::insert and spawn_player both borrow mutably
    let player_ent = spawner::spawn_player(&mut gs.ecs, 0, 0);
    insert_all!(gs.ecs, player_ent);

    //Generate map
    gs.generate_world_map(1);

    //Start game
    main_loop(context, gs)
}
