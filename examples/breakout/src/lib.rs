//! Breakout game state updated in Rust and rendered by a Node.js terminal.
//!
//! JavaScript retains native references to the ball and bricks. Each simulation
//! step mutates those same objects, so rendering reads their current fields.

use wasmgc::{Gc, GcArray, wasmgc};

const WIDTH: f32 = 60.0;
const HEIGHT: f32 = 18.0;
const PADDLE_WIDTH: f32 = 10.0;
const PADDLE_Y: f32 = 17.0;
const BALL_RADIUS: f32 = 0.3;
const BRICK_WIDTH: f32 = 4.0;
const BRICK_HEIGHT: f32 = 1.0;
const BRICK_COUNT: u32 = 30;

const READY: u32 = 0;
const RUNNING: u32 = 1;
const WON: u32 = 2;
const LOST: u32 = 3;

/// The moving ball, with its position exposed to the renderer.
#[wasmgc]
pub struct Ball {
    /// Horizontal position in playfield coordinates.
    #[getter]
    pub x: f32,
    /// Vertical position, increasing down the playfield.
    #[getter]
    pub y: f32,
    vx: f32,
    vy: f32,
}

/// A brick that remains allocated after a hit and is marked inactive.
#[wasmgc]
pub struct Brick {
    /// Horizontal position in playfield coordinates.
    #[getter]
    pub x: f32,
    /// Vertical position, increasing down the playfield.
    #[getter]
    pub y: f32,
    /// Whether the brick still participates in rendering and collisions.
    #[getter]
    pub alive: bool,
}

/// Shared game state containing the ball and the array of bricks.
#[wasmgc]
pub struct Game {
    /// The ball object retained by the renderer.
    #[getter]
    pub ball: Gc<Ball>,
    /// All bricks, including those already hit.
    #[getter]
    pub bricks: GcArray<Gc<Brick>>,
    /// Horizontal position of the paddle's left edge.
    #[getter]
    pub paddle_x: f32,
    /// Points earned, with ten points awarded per brick.
    #[getter]
    pub score: u32,
    /// Remaining balls, including the one currently in play.
    #[getter]
    pub lives: u32,
    /// Phase: 0 is ready, 1 is running, 2 is won, and 3 is lost.
    #[getter]
    pub status: u32,
    remaining: u32,
}

/// Creates a game with 30 bricks, three lives, and a ball ready to launch.
#[wasmgc]
pub fn create_game() -> Gc<Game> {
    let bricks: Vec<_> = (0..BRICK_COUNT)
        .map(|index| {
            Gc::new(Brick {
                x: 1.0 + (index % 10) as f32 * 6.0,
                y: 2.0 + (index / 10) as f32 * 2.0,
                alive: true,
            })
        })
        .collect();

    Gc::new(Game {
        ball: Gc::new(Ball {
            x: WIDTH / 2.0,
            y: PADDLE_Y - BALL_RADIUS,
            vx: 12.0,
            vy: -14.0,
        }),
        bricks: GcArray::from_slice(bricks),
        paddle_x: (WIDTH - PADDLE_WIDTH) / 2.0,
        score: 0,
        lives: 3,
        status: READY,
        remaining: BRICK_COUNT,
    })
}

/// Returns a brick from the shared array.
///
/// # Panics
///
/// Panics if `index` is outside the array.
#[wasmgc]
pub fn brick_at(bricks: &GcArray<Gc<Brick>>, index: u32) -> Gc<Brick> {
    bricks
        .get(index as usize)
        .expect("brick index out of bounds")
}

/// Moves the paddle by `delta`, keeping it within the playfield.
///
/// Before launch, the ball follows the paddle.
#[wasmgc]
pub fn move_paddle(game: &Gc<Game>, delta: f32) {
    let x = (game.paddle_x() + delta).clamp(0.0, WIDTH - PADDLE_WIDTH);
    game.set_paddle_x(x);

    if game.status() == READY {
        game.ball().set_x(x + PADDLE_WIDTH / 2.0);
    }
}

/// Launches the waiting ball; does nothing during play or after game over.
#[wasmgc]
pub fn launch(game: &Gc<Game>) {
    if game.status() == READY {
        game.set_status(RUNNING);
    }
}

/// Advances a running game by at most 50 milliseconds.
///
/// `dt` is elapsed time in seconds. Other game phases remain unchanged.
#[wasmgc]
pub fn step(game: &Gc<Game>, dt: f32) {
    let ball = game.ball();
    let bricks = game.bricks();
    let mut remaining = dt.clamp(0.0, 0.05);

    // Cap movement per collision check so the ball cannot skip a thin brick
    // or the paddle when a rendered frame takes longer than a simulation step.
    while remaining > 0.0 && game.status() == RUNNING {
        let dt = remaining.min(1.0 / 240.0);
        remaining -= dt;
        advance(game, &ball, &bricks, dt);
    }
}

fn advance(game: &Gc<Game>, ball: &Gc<Ball>, bricks: &GcArray<Gc<Brick>>, dt: f32) {
    let old_x = ball.x();
    let old_y = ball.y();
    let mut vx = ball.vx();
    let mut vy = ball.vy();
    let mut x = old_x + vx * dt;
    let mut y = old_y + vy * dt;

    if x < BALL_RADIUS {
        x = BALL_RADIUS;
        vx = vx.abs();
    } else if x > WIDTH - BALL_RADIUS {
        x = WIDTH - BALL_RADIUS;
        vx = -vx.abs();
    }

    if y < BALL_RADIUS {
        y = BALL_RADIUS;
        vy = vy.abs();
    }

    let paddle_x = game.paddle_x();
    let paddle_top = PADDLE_Y - BALL_RADIUS;
    if vy > 0.0
        && old_y <= paddle_top
        && y >= paddle_top
        && x >= paddle_x - BALL_RADIUS
        && x <= paddle_x + PADDLE_WIDTH + BALL_RADIUS
    {
        y = paddle_top;
        vy = -14.0;
        let offset = ((x - paddle_x) / PADDLE_WIDTH * 2.0 - 1.0).clamp(-1.0, 1.0);
        vx = if offset.abs() < 0.15 {
            vx.signum() * 4.0
        } else {
            offset * 18.0
        };
    }

    for brick in bricks.iter() {
        if !brick.alive() {
            continue;
        }

        let left = brick.x() - BALL_RADIUS;
        let right = brick.x() + BRICK_WIDTH + BALL_RADIUS;
        let top = brick.y() - BALL_RADIUS;
        let bottom = brick.y() + BRICK_HEIGHT + BALL_RADIUS;
        if x < left || x > right || y < top || y > bottom {
            continue;
        }

        // Reflect on the entry face inferred from the previous position,
        // rather than selecting a face from the overlapping position alone.
        if old_y <= top {
            y = top;
            vy = -vy.abs();
        } else if old_y >= bottom {
            y = bottom;
            vy = vy.abs();
        } else if old_x <= left {
            x = left;
            vx = -vx.abs();
        } else {
            x = right;
            vx = vx.abs();
        }

        brick.set_alive(false);
        game.set_score(game.score() + 10);
        game.set_remaining(game.remaining() - 1);
        if game.remaining() == 0 {
            game.set_status(WON);
        }
        break;
    }

    ball.set_x(x);
    ball.set_y(y);
    ball.set_vx(vx);
    ball.set_vy(vy);

    if y > HEIGHT {
        let lives = game.lives() - 1;
        game.set_lives(lives);
        game.set_status(if lives == 0 { LOST } else { READY });
        if lives > 0 {
            ball.set_x(paddle_x + PADDLE_WIDTH / 2.0);
            ball.set_y(paddle_top);
            ball.set_vx(12.0);
            ball.set_vy(-14.0);
        }
    }
}
