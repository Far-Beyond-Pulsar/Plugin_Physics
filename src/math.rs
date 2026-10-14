//! Conversions between SceneDB's `Transform` and Rapier's types.

use rapier3d::prelude::{Pose, Rotation, Vector};

/// A `Transform`'s rotation (degrees, applied Y then X then Z as the
/// renderer does) as a unit quaternion `[x, y, z, w]`.
pub fn quat_from_euler_degrees(rotation: [f32; 3]) -> [f32; 4] {
    let [x, y, z] = rotation.map(|d| d.to_radians() * 0.5);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    // q = qy * qx * qz
    [
        cy * sx * cz + sy * cx * sz,
        sy * cx * cz - cy * sx * sz,
        cy * cx * sz - sy * sx * cz,
        cy * cx * cz + sy * sx * sz,
    ]
}

/// Inverse of [`quat_from_euler_degrees`].
pub fn euler_degrees_from_quat([x, y, z, w]: [f32; 4]) -> [f32; 3] {
    let sin_x = (2.0 * (w * x - y * z)).clamp(-1.0, 1.0);
    let pitch = sin_x.asin();
    let yaw = (2.0 * (x * z + w * y)).atan2(1.0 - 2.0 * (x * x + y * y));
    let roll = (2.0 * (x * y + w * z)).atan2(1.0 - 2.0 * (x * x + z * z));
    [pitch.to_degrees(), yaw.to_degrees(), roll.to_degrees()]
}

/// A pose from a `Transform`'s position and rotation (degrees).
pub fn pose(position: [f32; 3], rotation: [f32; 3]) -> Pose {
    let [x, y, z, w] = quat_from_euler_degrees(rotation);
    Pose::from_parts(vector(position), Rotation::from_xyzw(x, y, z, w))
}

pub fn vector(v: [f32; 3]) -> Vector {
    Vector::new(v[0], v[1], v[2])
}

pub fn array(v: Vector) -> [f32; 3] {
    [v.x, v.y, v.z]
}

/// Equal within float noise.
pub fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(&b).all(|(a, b)| (a - b).abs() <= 1e-5)
}

pub fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn euler_round_trips() {
        for r in [
            [0.0, 0.0, 0.0],
            [10.0, 20.0, 30.0],
            [-45.0, 170.0, 5.0],
            [80.0, -90.0, 0.0],
        ] {
            let back = euler_degrees_from_quat(quat_from_euler_degrees(r));
            let again = quat_from_euler_degrees(back);
            let q = quat_from_euler_degrees(r);
            // Same rotation (a quaternion and its negation are the same one).
            let dot: f32 = q.iter().zip(&again).map(|(a, b)| a * b).sum();
            assert!(dot.abs() > 0.9999, "{r:?} -> {back:?}");
        }
    }

    #[test]
    fn yaw_is_about_y() {
        let [x, y, z, w] = quat_from_euler_degrees([0.0, 90.0, 0.0]);
        let half = std::f32::consts::FRAC_1_SQRT_2;
        assert!((y - half).abs() < 1e-5 && (w - half).abs() < 1e-5);
        assert!(x.abs() < 1e-5 && z.abs() < 1e-5);
    }
}
