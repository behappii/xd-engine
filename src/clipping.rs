use crate::{math::Vec4, scene::EPSILON};

/// Плоскости отсечения
#[derive(Clone, Copy)]
pub enum Plane {
    Left,
    Right,
    Bottom,
    Top,
    Near,
    Far,
}

// Расстояние до плоскости
pub fn plane_distance(v: &Vec4, plane: Plane) -> f32 {
    match plane {
        Plane::Left => v.x + v.w,
        Plane::Right => v.w - v.x,

        Plane::Bottom => v.y + v.w,
        Plane::Top => v.w - v.y,

        Plane::Near => v.z + v.w,
        Plane::Far => v.w - v.z,
    }
}

// Интерполяция вершины
pub fn lerp_vec4(a: Vec4, b: Vec4, t: f32) -> Vec4 {
    Vec4 {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        z: a.z + (b.z - a.z) * t,
        w: a.w + (b.w - a.w) * t,
    }
}

// Клиппинг одной линии
pub fn clip_line_4d(mut v0: Vec4, mut v1: Vec4) -> Option<(Vec4, Vec4)> {
    let planes = [
        Plane::Left,
        Plane::Right,
        Plane::Bottom,
        Plane::Top,
        Plane::Near,
        Plane::Far,
    ];

    for plane in planes {
        let d0 = plane_distance(&v0, plane);
        let d1 = plane_distance(&v1, plane);

        let inside0 = d0 >= -EPSILON;
        let inside1 = d1 >= -EPSILON;

        match (inside0, inside1) {
            (true, true) => {}

            (false, false) => {
                return None;
            }

            _ => {
                let denom = d0 - d1;

                if denom.abs() < EPSILON {
                    return None;
                }

                let t = d0 / denom;

                let intersection = lerp_vec4(v0, v1, t);

                if inside0 {
                    v1 = intersection;
                } else {
                    v0 = intersection;
                }
            }
        }
    }

    Some((v0, v1))
}
