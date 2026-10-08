//! Free-fly camera: hold the right mouse button to look, WASD to move,
//! Q/E down/up, Shift for speed.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;

#[derive(Component)]
pub struct FlyCam {
    pub yaw: f32,
    pub pitch: f32,
    /// Metres per second.
    pub speed: f32,
}

impl Default for FlyCam {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: -0.3, speed: 6.0 }
    }
}

impl FlyCam {
    pub fn look_at(&mut self, tf: &mut Transform, from: Vec3, target: Vec3) {
        let d = (target - from).normalize_or(Vec3::NEG_Z);
        self.yaw = (-d.x).atan2(-d.z);
        self.pitch = d.y.asin();
        tf.translation = from;
        tf.rotation = Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0);
    }
}

pub fn fly(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut q: Query<(&mut Transform, &mut FlyCam)>,
) {
    let Ok((mut tf, mut cam)) = q.single_mut() else { return };
    if buttons.pressed(MouseButton::Right) {
        let d = motion.delta * 0.003;
        cam.yaw -= d.x;
        cam.pitch = (cam.pitch - d.y).clamp(-1.54, 1.54);
    }
    tf.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);

    let mut dir = Vec3::ZERO;
    let (fwd, right) = (tf.forward().as_vec3(), tf.right().as_vec3());
    for (key, v) in [
        (KeyCode::KeyW, fwd),
        (KeyCode::KeyS, -fwd),
        (KeyCode::KeyD, right),
        (KeyCode::KeyA, -right),
        (KeyCode::KeyE, Vec3::Y),
        (KeyCode::KeyQ, Vec3::NEG_Y),
    ] {
        if keys.pressed(key) {
            dir += v;
        }
    }
    let boost = if keys.pressed(KeyCode::ShiftLeft) { 5.0 } else { 1.0 };
    tf.translation += dir.normalize_or_zero() * cam.speed * boost * time.delta_secs();
}
