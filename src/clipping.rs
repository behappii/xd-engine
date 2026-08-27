use crate::{config::EPSILON, math::Vec4};

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

pub fn clip_triangle_near(triangle: [Vec4; 3]) -> ([[Vec4; 3]; 2], usize) {
    let zero = Vec4 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 0.0,
    };

    // Алгоритм Сазерленда — Ходжмана: обходим рёбра, копим вершины внутри
    let mut poly = [zero; 4];
    let mut count = 0usize;

    for i in 0..3 {
        let cur = triangle[i];
        let next = triangle[(i + 1) % 3];

        let d_cur = plane_distance(&cur, Plane::Near);
        let d_next = plane_distance(&next, Plane::Near);

        let inside_cur = d_cur >= 0.0;
        let inside_next = d_next >= 0.0;

        if inside_cur {
            poly[count] = cur;
            count += 1;
        }

        // Ребро пересекает плоскость — добавляем точку пересечения
        if inside_cur != inside_next {
            let denom = d_cur - d_next;
            if denom.abs() > EPSILON {
                poly[count] = lerp_vec4(cur, next, d_cur / denom);
                count += 1;
            }
        }
    }

    let mut out = [[zero; 3]; 2];

    match count {
        3 => {
            out[0] = [poly[0], poly[1], poly[2]];
            (out, 1)
        }
        4 => {
            out[0] = [poly[0], poly[1], poly[2]];
            out[1] = [poly[0], poly[2], poly[3]];
            (out, 2)
        }
        _ => (out, 0),
    }
}
