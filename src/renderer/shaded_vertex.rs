use crate::math::{Vec2, Vec3, Vec4};

/// Вершина на входе растеризатора: позиция в clip space плюс всё, что нужно
/// протянуть по треугольнику с интерполяцией.
///
/// Отдельный тип от `scene::Vertex`: там сырые атрибуты меша в локальных
/// координатах, здесь — уже обработанные, готовые к растеризации. Пара
/// «вершинный этап -> фрагментный этап» ровно та же, что в шейдерном
/// пайплайне на GPU, а `color` и `uv` здесь — это varying.
#[derive(Debug, Clone, Copy)]
pub struct ShadedVertex {
    pub clip_position: Vec4,
    /// Цвет уже с учётом освещения, компоненты в диапазоне 0..1
    pub color: Vec3,
    /// Текстурная координата. У меша без развёртки — нули: тогда вся грань
    /// читает один и тот же тексель, что безобидно, потому что текстуры
    /// у такого инстанса всё равно нет
    pub uv: Vec2,
}

impl ShadedVertex {
    pub fn new(clip_position: Vec4, color: Vec3) -> Self {
        Self {
            clip_position,
            color,
            uv: Vec2::ZERO,
        }
    }

    pub fn with_uv(mut self, uv: Vec2) -> Self {
        self.uv = uv;
        self
    }
}
