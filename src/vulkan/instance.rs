//! `VkInstance` — точка входа в Vulkan для конкретного приложения, плюс
//! таблица функций уровня instance, которые сам Vulkan требует доставать
//! через `vkGetInstanceProcAddr`, а не звать напрямую (см. `loader.rs`).

use crate::vulkan::ffi::*;
use crate::vulkan::loader::{Library, PfnVoidFunction, vk_load};
use std::ffi::{CStr, CString, c_void};
use std::os::raw::c_char;

/// Слой валидации — не обязателен для работы, но без него ошибки в наших
/// же вызовах (неверный `sType`, забытый обязательный параметр) будут
/// выглядеть как крах драйвера в случайном месте вместо понятного
/// сообщения в stderr. Включаем, только если он реально стоит на машине
/// (LunarG Vulkan SDK ставит его сам) — попытка включить отсутствующий
/// слой роняет `vkCreateInstance` целиком, а без слоя жить можно
const VALIDATION_LAYER: &CStr = c"VK_LAYER_KHRONOS_validation";

type PfnCreateInstance =
    unsafe extern "system" fn(*const VkInstanceCreateInfo, *const c_void, *mut VkInstance) -> VkEnum;
type PfnEnumerateInstanceLayerProperties =
    unsafe extern "system" fn(*mut u32, *mut VkLayerProperties) -> VkEnum;
type PfnDestroyInstance = unsafe extern "system" fn(VkInstance, *const c_void);
type PfnEnumeratePhysicalDevices =
    unsafe extern "system" fn(VkInstance, *mut u32, *mut VkPhysicalDevice) -> VkEnum;
type PfnGetPhysicalDeviceQueueFamilyProperties =
    unsafe extern "system" fn(VkPhysicalDevice, *mut u32, *mut VkQueueFamilyProperties);
type PfnGetPhysicalDeviceSurfaceSupportKHR =
    unsafe extern "system" fn(VkPhysicalDevice, u32, VkSurfaceKHR, *mut VkBool32) -> VkEnum;
type PfnGetPhysicalDeviceSurfaceCapabilitiesKHR =
    unsafe extern "system" fn(VkPhysicalDevice, VkSurfaceKHR, *mut VkSurfaceCapabilitiesKHR) -> VkEnum;
type PfnGetPhysicalDeviceSurfaceFormatsKHR =
    unsafe extern "system" fn(VkPhysicalDevice, VkSurfaceKHR, *mut u32, *mut VkSurfaceFormatKHR) -> VkEnum;
type PfnGetPhysicalDeviceSurfacePresentModesKHR =
    unsafe extern "system" fn(VkPhysicalDevice, VkSurfaceKHR, *mut u32, *mut VkEnum) -> VkEnum;
type PfnCreateDevice =
    unsafe extern "system" fn(VkPhysicalDevice, *const VkDeviceCreateInfo, *const c_void, *mut VkDevice) -> VkEnum;
pub type PfnGetDeviceProcAddr = unsafe extern "system" fn(VkDevice, *const c_char) -> Option<PfnVoidFunction>;
type PfnDestroySurfaceKHR = unsafe extern "system" fn(VkInstance, VkSurfaceKHR, *const c_void);
type PfnGetPhysicalDeviceMemoryProperties =
    unsafe extern "system" fn(VkPhysicalDevice, *mut VkPhysicalDeviceMemoryProperties);

/// Функции уровня instance, которые нужны где угодно после его создания:
/// выбор физического устройства (`device.rs`), создание поверхности
/// (`surface.rs`), сама раздача device-level функций (`vkGetDeviceProcAddr`,
/// используется в `device.rs`, чтобы получить уже быстрый путь к функциям
/// логического устройства — см. её doc-комментарий там)
pub struct InstanceFns {
    pub destroy_instance: PfnDestroyInstance,
    pub enumerate_physical_devices: PfnEnumeratePhysicalDevices,
    pub get_physical_device_queue_family_properties: PfnGetPhysicalDeviceQueueFamilyProperties,
    pub get_physical_device_surface_support_khr: PfnGetPhysicalDeviceSurfaceSupportKHR,
    pub get_physical_device_surface_capabilities_khr: PfnGetPhysicalDeviceSurfaceCapabilitiesKHR,
    pub get_physical_device_surface_formats_khr: PfnGetPhysicalDeviceSurfaceFormatsKHR,
    pub get_physical_device_surface_present_modes_khr: PfnGetPhysicalDeviceSurfacePresentModesKHR,
    pub create_device: PfnCreateDevice,
    pub get_device_proc_addr: PfnGetDeviceProcAddr,
    pub destroy_surface_khr: PfnDestroySurfaceKHR,
    pub get_physical_device_memory_properties: PfnGetPhysicalDeviceMemoryProperties,
}

pub struct Instance {
    pub handle: VkInstance,
    pub fns: InstanceFns,
}

impl Instance {
    /// `required_extensions` — платформенное расширение поверхности
    /// (`VK_EXT_metal_surface` / `VK_KHR_win32_surface` / `VK_KHR_xlib_surface`,
    /// см. `surface.rs`) плюс `VK_KHR_surface`, общее для всех платформ
    pub fn new(lib: &Library, app_name: &str, required_extensions: &[&CStr]) -> Result<Self, String> {
        let create_instance: PfnCreateInstance =
            vk_load!(lib, VkInstance::NULL, "vkCreateInstance", PfnCreateInstance);
        let enumerate_layers: PfnEnumerateInstanceLayerProperties = vk_load!(
            lib,
            VkInstance::NULL,
            "vkEnumerateInstanceLayerProperties",
            PfnEnumerateInstanceLayerProperties
        );

        let app_name_c = CString::new(app_name).unwrap();
        let engine_name_c = c"xd_engine";
        let app_info = VkApplicationInfo {
            s_type: VK_STRUCTURE_TYPE_APPLICATION_INFO,
            p_next: std::ptr::null(),
            p_application_name: app_name_c.as_ptr(),
            application_version: vk_make_api_version(0, 0, 1, 0),
            p_engine_name: engine_name_c.as_ptr(),
            engine_version: vk_make_api_version(0, 0, 1, 0),
            api_version: VK_API_VERSION_1_0,
        };

        let layer_present = validation_layer_available(enumerate_layers);
        let layers: Vec<*const std::os::raw::c_char> =
            if layer_present { vec![VALIDATION_LAYER.as_ptr()] } else { Vec::new() };
        if !layer_present {
            eprintln!(
                "Vulkan: слой {:?} не найден, продолжаем без валидации — ошибки в наших вызовах API будут вылезать не сообщением, а крахом или странной картинкой",
                VALIDATION_LAYER
            );
        }

        let extensions: Vec<*const std::os::raw::c_char> =
            required_extensions.iter().map(|e| e.as_ptr()).collect();

        let create_info = VkInstanceCreateInfo {
            s_type: VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: 0,
            p_application_info: &app_info,
            enabled_layer_count: layers.len() as u32,
            pp_enabled_layer_names: layers.as_ptr(),
            enabled_extension_count: extensions.len() as u32,
            pp_enabled_extension_names: extensions.as_ptr(),
        };

        let mut handle = VkInstance::NULL;
        let result = unsafe { create_instance(&create_info, std::ptr::null(), &mut handle) };
        if result != VK_SUCCESS {
            return Err(format!("vkCreateInstance вернул {result}"));
        }

        // `vkDestroyInstance` достаётся ПЕРВЫМ и отдельно от остальной
        // таблицы: с этого момента у нас есть чем убрать уже созданный
        // instance, если не найдётся любая следующая функция. Раньше вся
        // таблица собиралась одним литералом, а `vk_load!` внутри делает
        // `return Err` — то есть первая же ненайденная функция уносила
        // управление из `new`, оставляя живой `VkInstance` навсегда.
        //
        // Обойти самый первый случай нечем: если не нашлась сама
        // `vkDestroyInstance`, уничтожать instance попросту некому. Но это
        // и не потеря — библиотека без `vkDestroyInstance` не Vulkan вовсе,
        // и такой процесс всё равно сейчас же завершится ошибкой
        let destroy_instance: PfnDestroyInstance =
            vk_load!(lib, handle, "vkDestroyInstance", PfnDestroyInstance);
        let fns = match load_instance_fns(lib, handle, destroy_instance) {
            Ok(fns) => fns,
            Err(err) => {
                unsafe {
                    destroy_instance(handle, std::ptr::null());
                }
                return Err(err);
            }
        };

        Ok(Self { handle, fns })
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // Обратный порядок инициализации: сначала должны быть уничтожены
        // все дочерние объекты (device, surface — см. `Drop` у `Renderer`
        // в `context.rs`, там порядок полей и решает порядок уничтожения),
        // instance закрывается последним
        unsafe {
            (self.fns.destroy_instance)(self.handle, std::ptr::null());
        }
    }
}

/// Остальная таблица функций уровня instance — отдельной функцией, а не
/// литералом прямо в `Instance::new`, ровно ради корректной уборки: `vk_load!`
/// на неудаче делает `return Err`, и пока всё это лежало в `new`, такой выход
/// пропускал уничтожение уже созданного `VkInstance`. Здесь ранний выход
/// возвращает управление вызывающей стороне, которой есть чем прибраться
fn load_instance_fns(
    lib: &Library,
    handle: VkInstance,
    destroy_instance: PfnDestroyInstance,
) -> Result<InstanceFns, String> {
    Ok(InstanceFns {
        destroy_instance,
        enumerate_physical_devices: vk_load!(
            lib,
            handle,
            "vkEnumeratePhysicalDevices",
            PfnEnumeratePhysicalDevices
        ),
        get_physical_device_queue_family_properties: vk_load!(
            lib,
            handle,
            "vkGetPhysicalDeviceQueueFamilyProperties",
            PfnGetPhysicalDeviceQueueFamilyProperties
        ),
        get_physical_device_surface_support_khr: vk_load!(
            lib,
            handle,
            "vkGetPhysicalDeviceSurfaceSupportKHR",
            PfnGetPhysicalDeviceSurfaceSupportKHR
        ),
        get_physical_device_surface_capabilities_khr: vk_load!(
            lib,
            handle,
            "vkGetPhysicalDeviceSurfaceCapabilitiesKHR",
            PfnGetPhysicalDeviceSurfaceCapabilitiesKHR
        ),
        get_physical_device_surface_formats_khr: vk_load!(
            lib,
            handle,
            "vkGetPhysicalDeviceSurfaceFormatsKHR",
            PfnGetPhysicalDeviceSurfaceFormatsKHR
        ),
        get_physical_device_surface_present_modes_khr: vk_load!(
            lib,
            handle,
            "vkGetPhysicalDeviceSurfacePresentModesKHR",
            PfnGetPhysicalDeviceSurfacePresentModesKHR
        ),
        create_device: vk_load!(lib, handle, "vkCreateDevice", PfnCreateDevice),
        get_device_proc_addr: vk_load!(lib, handle, "vkGetDeviceProcAddr", PfnGetDeviceProcAddr),
        destroy_surface_khr: vk_load!(lib, handle, "vkDestroySurfaceKHR", PfnDestroySurfaceKHR),
        get_physical_device_memory_properties: vk_load!(
            lib,
            handle,
            "vkGetPhysicalDeviceMemoryProperties",
            PfnGetPhysicalDeviceMemoryProperties
        ),
    })
}

/// Проверяет, стоит ли на машине слой валидации, не полагаясь на удачу:
/// `vkEnumerateInstanceLayerProperties` зовут дважды — сперва узнать
/// количество (Vulkan так делает почти везде, где отдаёт переменной длины
/// массив: одна функция на «сколько» и «дай сами данные», а не отдельная
/// функция подсчёта)
fn validation_layer_available(enumerate: PfnEnumerateInstanceLayerProperties) -> bool {
    let mut count = 0u32;
    unsafe {
        enumerate(&mut count, std::ptr::null_mut());
    }
    if count == 0 {
        return false;
    }

    let mut layers = vec![
        VkLayerProperties {
            layer_name: [0; 256],
            spec_version: 0,
            implementation_version: 0,
            description: [0; 256],
        };
        count as usize
    ];
    unsafe {
        enumerate(&mut count, layers.as_mut_ptr());
    }

    layers.iter().any(|layer| {
        let name = unsafe { CStr::from_ptr(layer.layer_name.as_ptr()) };
        name == VALIDATION_LAYER
    })
}
