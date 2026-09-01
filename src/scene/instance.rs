use crate::{
    config::LINE_COLOR,
    math::{Mat4, Vec3},
};

use super::{MeshId, TextureId};

pub struct Instance {
    pub mesh: MeshId,
    pub position: Vec3,
    pub rotation: Vec3,
    pub scale: Vec3,

    // цвет всего объекта
    pub color: [u8; 4], // R G B A
    // необязательная раскраска по треугольникам
    // индекс совпадает с mesh.triangles
    pub face_colors: Option<Vec<[u8; 4]>>,

    /// Текстура объекта, тоже ссылкой в арену сцены: одну картинку разделяют
    /// десятки инстансов, копировать её на каждый незачем
    pub texture: Option<TextureId>,

    pub wireframe: bool,
}

impl Instance {
    pub fn new(mesh: MeshId, position: Vec3) -> Self {
        Self {
            mesh,
            position,
            rotation: Vec3::new(0.0, 0.0, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            color: LINE_COLOR,
            face_colors: None,
            texture: None,
            wireframe: false,
        }
    }
    /// Задать цвет всему инстансу
    pub fn with_color(mut self, color: [u8; 4]) -> Self {
        self.color = color;
        self
    }
    /// Раскраска по индексу треугольников
    pub fn with_face_colors(mut self, colors: Vec<[u8; 4]>) -> Self {
        self.face_colors = Some(colors);
        self
    }

    /// Натянуть текстуру.
    ///
    /// Цвет инстанса при этом не отменяется, а умножается на тексель: белый
    /// (255, 255, 255) отдаёт картинку как есть, любой другой её подкрашивает.
    /// Смысла в текстуре не будет, если у меша нет развёртки — тогда все UV
    /// нулевые и вся поверхность прочитает один и тот же левый верхний тексель
    pub fn with_texture(mut self, texture: TextureId) -> Self {
        self.texture = Some(texture);
        self
    }

    pub fn as_wireframe(mut self) -> Self {
        self.wireframe = true;
        self
    }

    pub fn get_model_matrix(&self) -> Mat4 {
        let scale_mat = Mat4::scaling(self.scale.x, self.scale.y, self.scale.z);
        let rot_x = Mat4::rotation_x(self.rotation.x);
        let rot_y = Mat4::rotation_y(self.rotation.y);
        let rot_z = Mat4::rotation_z(self.rotation.z);
        let trans_mat = Mat4::translation(self.position.x, self.position.y, self.position.z);

        let rotation = &rot_x * &(&rot_y * &rot_z);
        let model = &trans_mat * &(&rotation * &scale_mat);
        model
    }
}
