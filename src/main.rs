use bevy::{
    app::AppExit,
    core::FixedTimestep,
    input::{keyboard::KeyCode, keyboard::KeyboardInput, ElementState},
    prelude::*,
    sprite::collide_aabb::collide,
    tasks::IoTaskPool,
};

use bevy_ggrs::*;
use bytemuck::{Pod, Zeroable};
use ggrs::{Config, PlayerHandle};
use matchbox_socket::WebRtcSocket;
use rand::Rng;

const HEIGHT_BOXES: u32 = 20;
const WIDTH_BOXES: u32 = 10;
const BOX_SIZE: f32 = 26.;
const INPUT_SIZE: usize = std::mem::size_of::<u8>();
const ROLLBACK_DEFAULT: &str = "rollback_default";

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
enum AppState {
    Lobby,
    InGame,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct BoxInput {
    inp: u8,
}

#[derive(Debug)]
pub struct GGRSConfig;
impl Config for GGRSConfig {
    type Input = BoxInput;
    type State = u8;
    type Address = String;
}

enum CollisionEvent {
    Safe,
    Deadly,
}

#[derive(Component, Copy, Clone, Debug, Reflect)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Default for Direction {
    fn default() -> Direction {
        Direction::Up
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, StageLabel)]
struct FixedUpdateStage;

#[derive(Debug, Hash, PartialEq, Eq, Clone, StageLabel)]
struct SpawnFoodStage;

#[derive(Component)]
struct Head;

#[derive(Component, Default, Deref, DerefMut, Reflect)]
struct Snake(Vec<Entity>);

#[derive(Component, Copy, Clone, Debug, Default, Reflect)]
struct Segment {
    curr_dir: Direction,
    next_dir: Direction,
}

impl Segment {
    fn new_sprite_bundle(x: f32, y: f32) -> SpriteBundle {
        SpriteBundle {
            sprite: Sprite {
                color: Color::rgb(0., 0., 0.),
                custom_size: Some(Vec2::new(BOX_SIZE, BOX_SIZE)),
                ..default()
            },
            transform: Transform::from_xyz(x, y, 0.),
            ..default()
        }
    }
}

#[derive(Component, Copy, Clone, Debug)]
struct Food;

impl Food {
    fn new_sprite_bundle(x: f32, y: f32) -> SpriteBundle {
        SpriteBundle {
            sprite: Sprite {
                color: Color::rgb(255., 0., 0.),
                custom_size: Some(Vec2::new(BOX_SIZE, BOX_SIZE)),
                ..default()
            },
            transform: Transform::from_xyz(x, y, 0.),
            ..default()
        }
    }
}

fn main() {
    let mut app = App::new();
    GGRSPlugin::<GGRSConfig>::new()
        .with_update_frequency(60)
        .with_input_system(input)
        .register_rollback_type::<Transform>()
        .register_rollback_type::<Segment>()
        .register_rollback_type::<Snake>()
        .with_rollback_schedule(
            Schedule::default().with_stage(
                ROLLBACK_DEFAULT,
                SystemStage::parallel()
                    .with_run_criteria(FixedTimestep::step(0.10))
                    .with_system(move_snake)
                    .with_system(update_dir)
                    .with_system(check_collisions.after(move_snake))
                    .with_system(add_segment.after(check_collisions))
                    .with_system(game_over.after(check_collisions)),
            ),
        )
        .build(&mut app);

    app.insert_resource(WindowDescriptor {
        title: "Snek".to_string(),
        width: WIDTH_BOXES as f32 * BOX_SIZE,
        height: HEIGHT_BOXES as f32 * BOX_SIZE,
        resizable: false,
        ..default()
    })
    .insert_resource(Snake::default())
    .add_plugins(DefaultPlugins)
    .add_event::<CollisionEvent>()
    .add_startup_system(start_matchbox_socket)
    .add_startup_system(setup)
    .add_stage_after(
        CoreStage::Update,
        SpawnFoodStage,
        SystemStage::parallel()
            .with_run_criteria(FixedTimestep::step(2.0))
            .with_system(spawn_food),
    )
    .run();
}

fn start_matchbox_socket(mut commands: Commands, task_pool: Res<IoTaskPool>) {
    let room_url = "ws://127.0.0.1:3536/next_2";
    info!("Connecting to matchbox to server: {}", room_url);
    let (socket, message_loop) = WebRtcSocket::new(room_url);
    task_pool.spawn(message_loop).detach();
    commands.insert_resource(Some(socket));
}

fn setup(mut commands: Commands, mut snake: ResMut<Snake>) {
    commands.spawn_bundle(OrthographicCameraBundle::new_2d());
    // TODO: Fix collide
    /* Wall::boundary_walls()
    .iter()
    .cloned()
    .for_each(|(wall, sprite)| {
        commands.spawn_bundle(sprite).insert(wall);
    }); */
    *snake = Snake(vec![commands
        .spawn_bundle(Segment::new_sprite_bundle(BOX_SIZE / 2., BOX_SIZE / 2.))
        .insert(Segment {
            curr_dir: Direction::Up,
            next_dir: Direction::Up,
        })
        .insert(Head)
        .id()]);
}

fn input(
    _handle: In<PlayerHandle>,
    mut head_query: Query<&mut Segment, With<Head>>,
    mut key_events: EventReader<KeyboardInput>,
) -> BoxInput {
    let mut head_seg = head_query.single_mut();
    for key in key_events
        .iter()
        .filter(|event| matches!(event.state, ElementState::Pressed))
        .filter(|event| matches!(event.key_code, Some(_)))
        .map(|event| event.key_code.unwrap())
    {
        match (key, head_seg.curr_dir) {
            (KeyCode::Up, Direction::Down) => (),
            (KeyCode::Left, Direction::Right) => (),
            (KeyCode::Down, Direction::Up) => (),
            (KeyCode::Right, Direction::Left) => (),
            (KeyCode::Up, _) => {
                head_seg.next_dir = Direction::Up;
            }
            (KeyCode::Left, _) => {
                head_seg.next_dir = Direction::Left;
            }
            (KeyCode::Down, _) => {
                head_seg.next_dir = Direction::Down;
            }
            (KeyCode::Right, _) => {
                head_seg.next_dir = Direction::Right;
            }
            _ => (),
        }
    }
    let mut input: u8 = 0;
    BoxInput { inp: input }
}

fn update_dir(
    mut head_query: Query<&mut Segment, With<Head>>,
    mut key_events: EventReader<KeyboardInput>,
) {
    let mut head_seg = head_query.single_mut();
    for key in key_events
        .iter()
        .filter(|event| matches!(event.state, ElementState::Pressed))
        .filter(|event| matches!(event.key_code, Some(_)))
        .map(|event| event.key_code.unwrap())
    {
        match (key, head_seg.curr_dir) {
            (KeyCode::Up, Direction::Down) => (),
            (KeyCode::Left, Direction::Right) => (),
            (KeyCode::Down, Direction::Up) => (),
            (KeyCode::Right, Direction::Left) => (),
            (KeyCode::Up, _) => {
                head_seg.next_dir = Direction::Up;
            }
            (KeyCode::Left, _) => {
                head_seg.next_dir = Direction::Left;
            }
            (KeyCode::Down, _) => {
                head_seg.next_dir = Direction::Down;
            }
            (KeyCode::Right, _) => {
                head_seg.next_dir = Direction::Right;
            }
            _ => (),
        }
    }
}

fn move_snake(mut segment_query: Query<(&mut Segment, &mut Transform)>, snake: ResMut<Snake>) {
    if snake.len() > 1 {
        let snake_transforms = snake
            .iter()
            .map(|seg| {
                let (seg, trans) = segment_query.get_mut(*seg).unwrap();
                (*seg, *trans)
            })
            .collect::<Vec<_>>();

        snake_transforms
            .iter()
            .zip(snake.iter().skip(1))
            .for_each(|(first, second)| {
                let (first_seg, first_trans) = first;
                let (mut sec_seg, mut sec_trans) = segment_query.get_mut(*second).unwrap();
                *sec_seg = *first_seg;
                *sec_trans = *first_trans;
            });
    }

    let (mut head_seg, mut head_transform) =
        segment_query.get_mut(*snake.first().unwrap()).unwrap();
    match head_seg.next_dir {
        Direction::Up => head_transform.translation.y += BOX_SIZE,
        Direction::Down => head_transform.translation.y -= BOX_SIZE,
        Direction::Right => head_transform.translation.x += BOX_SIZE,
        Direction::Left => head_transform.translation.x -= BOX_SIZE,
    }
    head_seg.curr_dir = head_seg.next_dir;
}

fn check_collisions(
    mut commands: Commands,
    head_query: Query<&Transform, (With<Segment>, With<Head>)>,
    segment_query: Query<&Transform, (With<Segment>, Without<Head>)>,
    food_query: Query<(Entity, &Transform), With<Food>>,
    mut collision_events: EventWriter<CollisionEvent>,
) {
    let head_transform = head_query.single();
    if head_transform.translation.x.abs() >= BOX_SIZE * WIDTH_BOXES as f32 / 2.
        || head_transform.translation.y.abs() >= BOX_SIZE * HEIGHT_BOXES as f32 / 2.
    {
        collision_events.send(CollisionEvent::Deadly);
    }

    for seg_transform in segment_query.iter() {
        let collision = collide(
            head_transform.translation,
            head_transform.scale.truncate(),
            seg_transform.translation,
            seg_transform.scale.truncate(),
        );

        if let Some(_) = collision {
            collision_events.send(CollisionEvent::Deadly);
        }
    }
    for (food_entity, food_transform) in food_query.iter() {
        let collision = collide(
            head_transform.translation,
            head_transform.scale.truncate(),
            food_transform.translation,
            food_transform.scale.truncate(),
        );

        if let Some(_) = collision {
            collision_events.send(CollisionEvent::Safe);
            commands.entity(food_entity).despawn();
        }
    }
}

fn add_segment(
    mut commands: Commands,
    mut segment_query: Query<(&mut Segment, &mut Transform)>,
    mut collision_events: EventReader<CollisionEvent>,
    mut snake: ResMut<Snake>,
) {
    for event in collision_events.iter() {
        if let CollisionEvent::Safe = event {
            let (tail_seg, tail_trans) = segment_query.get_mut(*snake.last().unwrap()).unwrap();
            let tail_pos = tail_trans.translation;
            let (new_x, new_y) = match tail_seg.curr_dir {
                Direction::Up => (tail_pos.x, tail_pos.y - BOX_SIZE),
                Direction::Down => (tail_pos.x, tail_pos.y + BOX_SIZE),
                Direction::Left => (tail_pos.x + BOX_SIZE, tail_pos.y),
                Direction::Right => (tail_pos.x - BOX_SIZE, tail_pos.y),
            };
            snake.push(
                commands
                    .spawn_bundle(Segment::new_sprite_bundle(new_x, new_y))
                    .insert(*tail_seg)
                    .id(),
            );
        }
    }
}

fn spawn_food(mut commands: Commands, transform_query: Query<&Transform>) {
    loop {
        let x_pos = BOX_SIZE
            * rand::thread_rng()
                .gen_range::<i32, _>((-1 * WIDTH_BOXES as i32 / 2)..(WIDTH_BOXES as i32 / 2))
                as f32
            + BOX_SIZE / 2.;
        let y_pos = BOX_SIZE
            * rand::thread_rng()
                .gen_range::<i32, _>((-1 * HEIGHT_BOXES as i32 / 2)..(HEIGHT_BOXES as i32 / 2))
                as f32
            + BOX_SIZE / 2.;

        if transform_query.iter().count() as u32 >= WIDTH_BOXES * HEIGHT_BOXES {
            break;
        }
        if transform_query
            .iter()
            .filter(|trans| (trans.translation.x == x_pos && trans.translation.y == y_pos))
            .count()
            == 0
        {
            commands
                .spawn_bundle(Food::new_sprite_bundle(x_pos, y_pos))
                .insert(Food);
            break;
        }
    }
}

fn game_over(
    mut collision_events: EventReader<CollisionEvent>,
    mut app_exit_events: EventWriter<AppExit>,
) {
    for collision in collision_events.iter() {
        if let CollisionEvent::Deadly = collision {
            app_exit_events.send(AppExit);
        }
    }
}
