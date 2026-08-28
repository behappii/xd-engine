use crate::{config::EPSILON, math::Vec4, renderer::ShadedVertex};

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

/// Интерполяция вершины со всеми её атрибутами.
///
/// Важно, что это происходит ДО перспективного деления: в clip space атрибут
/// вдоль ребра меняется линейно, поэтому обычный lerp здесь корректен. Тот же
/// lerp после деления был бы уже неверен — там нужна поправка на w
pub fn lerp_shaded(a: ShadedVertex, b: ShadedVertex, t: f32) -> ShadedVertex {
    ShadedVertex {
        clip_position: lerp_vec4(a.clip_position, b.clip_position, t),
        color: a.color + (b.color - a.color) * t,
    }
}

pub fn clip_triangle_near(triangle: [ShadedVertex; 3]) -> ([[ShadedVertex; 3]; 2], usize) {
    // Заполнитель для ещё не заполненных ячеек — просто копия первой вершины:
    // осмысленного «нуля» у вершины с атрибутами нет
    let filler = triangle[0];

    // Алгоритм Сазерленда — Ходжмана: обходим рёбра, копим вершины внутри
    let mut poly = [filler; 4];
    let mut count = 0usize;

    for i in 0..3 {
        let cur = triangle[i];
        let next = triangle[(i + 1) % 3];

        let d_cur = plane_distance(&cur.clip_position, Plane::Near);
        let d_next = plane_distance(&next.clip_position, Plane::Near);

        let inside_cur = d_cur >= 0.0;
        let inside_next = d_next >= 0.0;

        if inside_cur {
            poly[count] = cur;
            count += 1;
        }

        // Ребро пересекает плоскость — добавляем точку пересечения.
        // Цвет новой вершины берётся тем же lerp, иначе на срезе был бы скачок
        if inside_cur != inside_next {
            let denom = d_cur - d_next;
            if denom.abs() > EPSILON {
                poly[count] = lerp_shaded(cur, next, d_cur / denom);
                count += 1;
            }
        }
    }

    let mut out = [[filler; 3]; 2];

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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::math::Vec3;

    /// Вершина в clip space с w = 1: тогда «внутри near» означает просто z >= -1
    fn v(x: f32, y: f32, z: f32) -> Vec4 {
        Vec4 { x, y, z, w: 1.0 }
    }

    /// То же, но с атрибутом: серый цвет заданной яркости
    fn shaded(x: f32, y: f32, z: f32, brightness: f32) -> ShadedVertex {
        ShadedVertex::new(v(x, y, z), Vec3::new(brightness, brightness, brightness))
    }

    #[test]
    fn triangle_fully_inside_passes_through_unchanged() {
        let (_, count) = clip_triangle_near([
            shaded(0.0, 0.0, 0.0, 1.0),
            shaded(1.0, 0.0, 0.0, 1.0),
            shaded(0.0, 1.0, 0.0, 1.0),
        ]);

        assert_eq!(count, 1);
    }

    #[test]
    fn triangle_fully_behind_near_plane_is_dropped() {
        let (_, count) = clip_triangle_near([
            shaded(0.0, 0.0, -5.0, 1.0),
            shaded(1.0, 0.0, -5.0, 1.0),
            shaded(0.0, 1.0, -5.0, 1.0),
        ]);

        assert_eq!(count, 0);
    }

    #[test]
    fn one_vertex_outside_splits_into_two_triangles() {
        // Отсечение четырёхугольника: две вершины внутри + две точки пересечения,
        // которые нельзя отрисовать одним треугольником
        let (_, count) = clip_triangle_near([
            shaded(0.0, 0.0, 0.0, 1.0),
            shaded(1.0, 0.0, 0.0, 1.0),
            shaded(0.0, 1.0, -5.0, 1.0),
        ]);

        assert_eq!(count, 2);
    }

    #[test]
    fn clipped_vertices_land_exactly_on_the_near_plane() {
        let (triangles, count) = clip_triangle_near([
            shaded(0.0, 0.0, 0.0, 1.0),
            shaded(1.0, 0.0, 0.0, 1.0),
            shaded(0.0, 1.0, -5.0, 1.0),
        ]);

        // Ни одна вершина результата не должна остаться за плоскостью, иначе
        // дальше случится деление на w <= 0 и на экране будет мусор
        for triangle in &triangles[..count] {
            for vertex in triangle {
                assert!(
                    plane_distance(&vertex.clip_position, Plane::Near) >= -EPSILON,
                    "вершина {:?} осталась за near",
                    vertex
                );
            }
        }
    }

    #[test]
    fn clipping_interpolates_attributes_along_the_cut_edge() {
        // Ребро от яркой вершины внутри к тёмной снаружи. Новая вершина
        // рождается на самой плоскости, и её цвет обязан быть промежуточным:
        // иначе на срезе был бы скачок яркости на ровном месте
        let (triangles, count) = clip_triangle_near([
            shaded(0.0, 0.0, 0.0, 1.0),
            shaded(1.0, 0.0, 0.0, 1.0),
            shaded(0.0, 1.0, -5.0, 0.0),
        ]);

        assert!(count > 0);

        let mut saw_intermediate = false;

        for triangle in &triangles[..count] {
            for vertex in triangle {
                let brightness = vertex.color.x;

                assert!(
                    (-EPSILON..=1.0 + EPSILON).contains(&brightness),
                    "яркость {} вылезла за пределы отрезка между концами ребра",
                    brightness
                );

                if brightness > EPSILON && brightness < 1.0 - EPSILON {
                    saw_intermediate = true;
                }
            }
        }

        assert!(saw_intermediate, "ни одна вершина среза не получила промежуточный цвет");
    }

    #[test]
    fn line_outside_a_single_plane_is_rejected() {
        // Оба конца правее правой плоскости (x > w) — рисовать нечего
        assert!(clip_line_4d(v(5.0, 0.0, 0.0), v(6.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn line_crossing_a_plane_is_shortened_to_the_boundary() {
        let (a, b) = clip_line_4d(v(0.0, 0.0, 0.0), v(5.0, 0.0, 0.0)).unwrap();

        // Начало внутри — не трогаем; конец подтянут к x = w = 1
        assert!((a.x - 0.0).abs() < EPSILON);
        assert!((b.x - 1.0).abs() < EPSILON);
    }
}
