//! Зеркало [`Assets`] на стороне GPU: вершинный и индексный буфер на каждый
//! меш, картинка с сэмплером и дескриптором на каждую текстуру.
//!
//! **Почему зеркало, а не общий тип.** У `Assets` и у этой арены разная
//! природа: там `Vec<Mesh>` — обычные данные, живущие сколько угодно и
//! копируемые как угодно, здесь — хендлы Vulkan, которые обязаны быть созданы
//! на конкретном `VkDevice` и уничтожены строго до него. Слить их в один тип
//! значило бы затащить Vulkan в `scene`, то есть в CPU-путь, которого эта
//! работа не касается вовсе.
//!
//! **Ключ — тот же индекс.** `MeshId`/`TextureId` внутри просто номер в
//! арене, и номер в наших `Vec` совпадает с ним по построению: мы идём по
//! `assets.meshes()` подряд и подряд же складываем результат. Никакой карты
//! соответствия поэтому нет — и заводить её не надо, пока арены только
//! растут.
//!
//! **Синхронизация ленивая, по длине.** [`GpuAssets::sync`] сравнивает
//! длины и догружает хвост. Игре не нужно ничего звать руками: положил меш в
//! `Assets` — на следующем кадре он окажется на видеокарте. Это работает
//! ровно потому, что `add_mesh`/`add_texture` — это `push`, и ничего никогда
//! не удаляется.
//!
//! **Известный пробел ровно оттуда же.** [`Assets::mesh_mut`] правит меш НА
//! МЕСТЕ, длина при этом не меняется — и GPU-копия останется старой. То есть
//! деформация вершин будет видна на CPU-пути и не видна на GPU. Лечится
//! счётчиком версий на арене (правка увеличивает — `sync` замечает); пока в
//! движке никто так не делает, и платить за это в каждом кадре незачем.

use crate::scene::{Assets, MeshId, Mesh, TextureId};
use crate::texture::{Magnify, Minify};
use crate::vulkan::buffer::{self, Buffer};
use crate::vulkan::descriptor;
use crate::vulkan::device::Device;
use crate::vulkan::ffi::*;
use crate::vulkan::image::GpuImage;
use crate::vulkan::pipeline::GpuVertex;
use crate::vulkan::sampler;

/// Геометрия одного меша на видеокарте
pub struct GpuMesh {
    pub vertex: Buffer,
    pub index: Buffer,
    pub index_count: u32,
}

/// Картинка одного меша на видеокарте вместе со своим сэмплером.
///
/// `set` заполняется не здесь, а при пересборке пула: наборы живут в пуле, а
/// он общий на всю арену и переживает не каждую текстуру, а всю их череду
pub struct GpuTexture {
    pub image: GpuImage,
    pub sampler: VkSampler,
    pub set: VkDescriptorSet,
}

impl GpuTexture {
    fn destroy(&self, device: &Device) {
        unsafe {
            (device.fns.destroy_sampler)(device.handle, self.sampler, std::ptr::null());
        }
        self.image.destroy(device);
    }
}

pub struct GpuAssets {
    /// `None` — у меша нет ни одного треугольника. Буфер нулевого размера
    /// создать нельзя (`vkCreateBuffer` такого не принимает), а ронять кадр
    /// из-за пустого меша незачем: он просто не рисуется
    meshes: Vec<Option<GpuMesh>>,
    textures: Vec<GpuTexture>,
    /// Белый тексель 1×1 для инстансов без текстуры — см. `descriptor_set`
    white: GpuTexture,
    pool: VkDescriptorPool,
    set_layout: VkDescriptorSetLayout,
}

impl GpuAssets {
    /// Пустая арена плюс белая заглушка — единственное, что здесь есть до
    /// первой синхронизации
    pub fn new(
        device: &Device,
        memory_properties: &VkPhysicalDeviceMemoryProperties,
        command_pool: VkCommandPool,
        set_layout: VkDescriptorSetLayout,
    ) -> Result<Self, String> {
        // Белый тексель, а не «отсутствие текстуры»: фрагментный шейдер
        // множит тексель на цвет покомпонентно, и белый — тождественная
        // операция (CLAUDE.md, «Текстура умножается на свет, а не заменяет
        // его»). Инстанс без текстуры получает эту заглушку и проходит тем же
        // единственным путём — без ветки в шейдере, без второго пайплайна и
        // без второго дескриптор-лейаута
        let image = GpuImage::upload_rgba8(device, memory_properties, command_pool, 1, 1, &[255, 255, 255, 255])?;
        let sampler = match sampler::create(device, Magnify::Nearest, Minify::Nearest) {
            Ok(sampler) => sampler,
            Err(err) => {
                image.destroy(device);
                return Err(err);
            }
        };

        let mut assets = Self {
            meshes: Vec::new(),
            textures: Vec::new(),
            white: GpuTexture { image, sampler, set: VkDescriptorSet::NULL },
            pool: VkDescriptorPool::NULL,
            set_layout,
        };

        if let Err(err) = assets.rebuild_descriptors(device) {
            assets.destroy(device);
            return Err(err);
        }

        Ok(assets)
    }

    /// Догрузить всё, что появилось в аренах с прошлого раза.
    ///
    /// Зовётся каждый кадр и в подавляющем большинстве кадров не делает
    /// ничего: два сравнения длин. Платить за это отдельным вызовом «а теперь
    /// загрузи ресурсы» со стороны игры не стоит — забыть его было бы легко,
    /// а симптом («новый объект не рисуется») уводил бы куда угодно
    pub fn sync(
        &mut self,
        device: &Device,
        memory_properties: &VkPhysicalDeviceMemoryProperties,
        command_pool: VkCommandPool,
        assets: &Assets,
    ) -> Result<(), String> {
        for mesh in &assets.meshes()[self.meshes.len()..] {
            self.meshes.push(upload_mesh(device, memory_properties, mesh)?);
        }

        let new_textures = &assets.textures()[self.textures.len()..];
        if new_textures.is_empty() {
            return Ok(());
        }

        for texture in new_textures {
            let pixels = texture.level0_rgba8();
            let image = GpuImage::upload_rgba8(
                device,
                memory_properties,
                command_pool,
                texture.width(),
                texture.height(),
                &pixels,
            )?;
            let sampler = match sampler::create(device, texture.magnify(), texture.minify()) {
                Ok(sampler) => sampler,
                Err(err) => {
                    image.destroy(device);
                    return Err(err);
                }
            };

            self.textures.push(GpuTexture { image, sampler, set: VkDescriptorSet::NULL });
        }

        // Наборы появившимся текстурам взять неоткуда: пул рассчитан ровно на
        // прежнее их число. Значит пересобираем его целиком и переписываем
        // ВСЕ наборы, включая уже работавшие — их хендлы после пересоздания
        // пула недействительны
        self.rebuild_descriptors(device)
    }

    /// Геометрия инстанса. `None` — меш пустой, рисовать нечего
    pub fn mesh(&self, id: MeshId) -> Option<&GpuMesh> {
        self.meshes[id.index()].as_ref()
    }

    /// Дескриптор текстуры инстанса, а для инстанса без текстуры — белая
    /// заглушка (см. `new`)
    pub fn descriptor_set(&self, id: Option<TextureId>) -> VkDescriptorSet {
        match id {
            Some(id) => self.textures[id.index()].set,
            None => self.white.set,
        }
    }

    /// Пересоздать пул под текущее число текстур и заново вписать все наборы.
    ///
    /// `vkDeviceWaitIdle` перед уничтожением обязателен: старые наборы могли
    /// быть привязаны в командном буфере кадра, который GPU ещё выполняет.
    /// Ждать целую очередь дорого, но случается это на загрузке ресурсов, а
    /// не в кадровом цикле — та же логика, что у одноразового командного
    /// буфера в `image.rs`
    fn rebuild_descriptors(&mut self, device: &Device) -> Result<(), String> {
        unsafe {
            (device.fns.device_wait_idle)(device.handle);
            if self.pool != VkDescriptorPool::NULL {
                (device.fns.destroy_descriptor_pool)(device.handle, self.pool, std::ptr::null());
                self.pool = VkDescriptorPool::NULL;
            }
        }

        // +1 — на белую заглушку: она такой же полноправный набор, просто не
        // из арены
        let capacity = self.textures.len() as u32 + 1;
        self.pool = descriptor::create_pool(device, capacity)?;

        for texture in self.textures.iter_mut().chain(std::iter::once(&mut self.white)) {
            texture.set = descriptor::allocate_set(device, self.pool, self.set_layout)?;
            descriptor::write_set(device, texture.set, texture.image.view, texture.sampler);
        }

        Ok(())
    }

    /// Наборы уничтожать отдельно не нужно — они исчезают вместе с пулом.
    /// `set_layout` тоже не наш: он создан в `context.rs` и там же
    /// уничтожается, потому что на него ссылается ещё и layout пайплайна
    pub fn destroy(&mut self, device: &Device) {
        unsafe {
            (device.fns.destroy_descriptor_pool)(device.handle, self.pool, std::ptr::null());
        }

        for mesh in self.meshes.iter_mut().flatten() {
            mesh.vertex.destroy(device);
            mesh.index.destroy(device);
        }

        for texture in &self.textures {
            texture.destroy(device);
        }
        self.white.destroy(device);
    }
}

/// Меш движка → пара буферов на видеокарте.
///
/// `scene::Vertex` сюда не едет как есть: у него нет `#[repr(C)]`, и
/// раскладка его полей в памяти не обещана вовсе — см. doc-комментарий
/// [`GpuVertex`]. Поэтому вершины перекладываются по одной, а индексы
/// расширяются из `usize` в `u32`: `VK_INDEX_TYPE_UINT32` — это ровно
/// четыре байта, а `usize` на 64-битной машине восемь
fn upload_mesh(
    device: &Device,
    memory_properties: &VkPhysicalDeviceMemoryProperties,
    mesh: &Mesh,
) -> Result<Option<GpuMesh>, String> {
    if mesh.triangles.is_empty() || mesh.vertices.is_empty() {
        return Ok(None);
    }

    let vertices: Vec<GpuVertex> = mesh
        .vertices
        .iter()
        .map(|v| GpuVertex {
            position: [v.position.x, v.position.y, v.position.z],
            normal: [v.normal.x, v.normal.y, v.normal.z],
            uv: [v.uv.x, v.uv.y],
        })
        .collect();
    let indices: Vec<u32> = mesh.triangles.iter().flat_map(|t| t.iter().map(|&i| i as u32)).collect();

    let vertex = buffer::upload_data(device, memory_properties, VK_BUFFER_USAGE_VERTEX_BUFFER_BIT, &vertices)?;
    let index = match buffer::upload_data(device, memory_properties, VK_BUFFER_USAGE_INDEX_BUFFER_BIT, &indices) {
        Ok(index) => index,
        Err(err) => {
            let mut vertex = vertex;
            vertex.destroy(device);
            return Err(err);
        }
    };

    Ok(Some(GpuMesh { vertex, index, index_count: indices.len() as u32 }))
}
