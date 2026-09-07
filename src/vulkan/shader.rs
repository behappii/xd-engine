//! Шейдеры Фазы 1 — сырой SPIR-V, собранный вручную, без GLSL-компилятора.
//!
//! **Почему не просто массив магических чисел.** SPIR-V — двоичный формат:
//! каждая инструкция начинается словом `(word_count << 16) | opcode`, и
//! `word_count` обязан ТОЧНО совпадать с числом слов инструкции — ошибиться
//! на одно слово значит сдвинуть чтение всех последующих инструкций, и
//! результат — не «шейдер работает неправильно», а «модуль не грузится
//! вовсе» либо, того хуже, читается как мусор. Ручной подсчёт слов на
//! ~30 инструкциях — верный способ ошибиться. Поэтому здесь есть свой
//! маленький ассемблер (`SpirvBuilder`) — не компилятор GLSL (никакого
//! разбора текста, синтаксиса, оптимизаций), а просто счётчик ID и
//! автоматический подсчёт `word_count`: каждую инструкцию и её операнды
//! всё равно задаём руками, ровно как того и просил учебный дух проекта,
//! но арифметику, в которой легко промахнуться, отдаём коду.
//!
//! **Происхождение чисел.** Коды операций и констант (`OP_*`, `CAPABILITY_*`
//! и так далее) — из спецификации SPIR-V, воспроизведены по памяти: на
//! этой машине нет установленных инструментов Vulkan/SPIR-V, сверить было
//! не с чем. Та же оговорка, что в `ffi.rs` — первая проверка настоящая
//! только при первом запуске (`vkCreateShaderModule` укажет на ошибку
//! кодом результата, а слой валидации — понятным сообщением, если модуль
//! пройдёт базовый разбор, но нарушит более тонкое правило).
//!
//! **Что рисуют эти два шейдера (Фаза 3).** Вершинный читает позицию и
//! нормаль из вершинного буфера, переводит позицию в clip space через MVP
//! (Фаза 2), а нормаль — в мир через матрицу нормалей, и считает ту же
//! ламбертову яркость, что и CPU-путь (`scene::pipeline::shade_instance`):
//! `AMBIENT_LIGHT + (1 - AMBIENT_LIGHT) * max(dot(N, L), 0)`, умноженную на
//! (пока заглушечный, без материалов) оранжевый базовый цвет. Результат едет
//! во фрагментный шейдер интерполированным атрибутом — Гуро, только
//! интерполяцию по треугольнику теперь делает сам растеризатор GPU, а не
//! `lerp_shaded` этого движка.
//!
//! **Зачем здесь GLSL.std.450.** Нормировка трансформированной нормали
//! нужна для корректного освещения при любом масштабе инстанса, а `sqrt`
//! отсутствует в самом ядре SPIR-V — это намеренное решение спецификации:
//! трансцендентная математика вынесена в расширяемый набор инструкций,
//! которым пользуются вообще все бэкенды, компилирующие во что угодно в
//! SPIR-V (glslang, DXC, naga, rustc_codegen_spirv), а не только компиляторы
//! GLSL-текста. `OpExtInstImport "GLSL.std.450"` — это одна инструкция в
//! модуле, который всё так же вручную собирает `SpirvBuilder`; тот же
//! принцип, что вызов `sqrtf()` из написанного вручную ассемблера не делает
//! программу «не своей».
//!
//! **Фаза 4 добавляет текстуру.** UV едет третьим вершинным атрибутом,
//! проходит через вершинный шейдер БЕЗ преобразований (развёртка задана в
//! той же плоскости, что и сам атлас — матрицы тут ни при чём) и
//! интерполируется во фрагментный вторым атрибутом рядом с цветом Гуро.
//! Там `OpImageSampleImplicitLod` читает тексель через `OpTypeSampledImage`
//! (combined image sampler — `descriptor.rs`), и тексель умножается на цвет
//! ПОКОМПОНЕНТНО, тем же приёмом, что `Vec3 * Vec3` на CPU-пути (см.
//! «Текстура умножается на свет, а не заменяет его» в CLAUDE.md): белая
//! текстура обязана оставить освещение как есть.

use crate::vulkan::device::Device;
use crate::vulkan::ffi::{
    VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO, VK_SUCCESS, VkShaderModule, VkShaderModuleCreateInfo,
};

// ============================================================================
// Крошечный ассемблер
// ============================================================================

struct SpirvBuilder {
    words: Vec<u32>,
    next_id: u32,
}

impl SpirvBuilder {
    fn new() -> Self {
        let mut b = Self { words: Vec::new(), next_id: 1 };
        // Заголовок модуля: magic number SPIR-V, версия 1.0 (старший байт —
        // major, следующий — minor: `0x00_01_00_00`), magic number
        // генератора (0 — «безымянный», официального номера у нас нет),
        // bound — впишем в `finish()`, когда счётчик ID уже не изменится,
        // и зарезервированное слово, которое сама спецификация просит
        // оставить нулём
        b.words.extend_from_slice(&[0x0723_0203, 0x0001_0000, 0, 0, 0]);
        b
    }

    /// ID выдаём заранее, до того как инструкция, которая его ОПРЕДЕЛЯЕТ,
    /// вообще записана — сама SPIR-V этого требует для `OpEntryPoint`
    /// (он всегда стоит раньше `OpFunction`) и `OpDecorate` (всегда раньше
    /// типа/переменной, которую украшает), так что вперёд забегать
    /// приходится по-любому
    fn id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Пишет инструкцию: сама считает `word_count` по числу операндов —
    /// ровно то место, где ручной подсчёт чаще всего и промахивается
    fn inst(&mut self, opcode: u32, operands: &[u32]) {
        let word_count = 1 + operands.len() as u32;
        self.words.push((word_count << 16) | opcode);
        self.words.extend_from_slice(operands);
    }

    fn finish(mut self) -> Vec<u32> {
        self.words[3] = self.next_id; // bound: на единицу больше последнего выданного ID
        self.words
    }
}

/// Пакует строку в слова SPIR-V: UTF-8 байты, нуль-терминатор, паддинг
/// нулями до кратности 4 байт, младший байт — в младшем байте слова
/// (little-endian порядок задан спецификацией, а не платформой)
fn encode_string(s: &str) -> Vec<u32> {
    let mut bytes = s.as_bytes().to_vec();
    bytes.push(0);
    while bytes.len() % 4 != 0 {
        bytes.push(0);
    }
    bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

// Коды операций, которые реально используем — не весь SPIR-V, а ровно
// подмножество для двух этих шейдеров
const OP_EXT_INST_IMPORT: u32 = 11;
const OP_EXT_INST: u32 = 12;
const OP_CAPABILITY: u32 = 17;
const OP_MEMORY_MODEL: u32 = 14;
const OP_ENTRY_POINT: u32 = 15;
const OP_EXECUTION_MODE: u32 = 16;
const OP_TYPE_VOID: u32 = 19;
const OP_TYPE_INT: u32 = 21;
const OP_TYPE_FLOAT: u32 = 22;
const OP_TYPE_VECTOR: u32 = 23;
const OP_TYPE_IMAGE: u32 = 25;
const OP_TYPE_SAMPLED_IMAGE: u32 = 27;
const OP_TYPE_ARRAY: u32 = 28;
const OP_TYPE_STRUCT: u32 = 30;
const OP_TYPE_POINTER: u32 = 32;
const OP_TYPE_FUNCTION: u32 = 33;
const OP_CONSTANT: u32 = 43;
const OP_CONSTANT_COMPOSITE: u32 = 44;
const OP_FUNCTION: u32 = 54;
const OP_FUNCTION_END: u32 = 56;
const OP_VARIABLE: u32 = 59;
const OP_LOAD: u32 = 61;
const OP_STORE: u32 = 62;
const OP_ACCESS_CHAIN: u32 = 65;
const OP_DECORATE: u32 = 71;
const OP_MEMBER_DECORATE: u32 = 72;
const OP_COMPOSITE_CONSTRUCT: u32 = 80;
const OP_COMPOSITE_EXTRACT: u32 = 81;
const OP_FADD: u32 = 129;
const OP_FMUL: u32 = 133;
// Скалярное произведение — обычная арифметика, не трансцендентная функция,
// поэтому у него, в отличие от sqrt/normalize, есть собственный опкод в
// самом ядре SPIR-V, и `GLSL.std.450` для него не нужен
const OP_DOT: u32 = 148;
/// Выборка текселя из `OpTypeSampledImage` по нормализованным координатам,
/// без явного LOD — фрагментный шейдер сам вправе выбрать уровень
/// детализации (у нас он всё равно всегда один, см. `sampler.rs`)
const OP_IMAGE_SAMPLE_IMPLICIT_LOD: u32 = 87;
const OP_LABEL: u32 = 248;
const OP_RETURN: u32 = 253;

// Номера инструкций внутри набора GLSL.std.450 — не опкоды SPIR-V, а
// literal-число четвёртым операндом OpExtInst (см. `OP_EXT_INST` выше).
// Как и коды операций SPIR-V в этом файле, воспроизведены по памяти без
// установленного Vulkan SDK
const GLSL_STD_450_FMAX: u32 = 40;
const GLSL_STD_450_NORMALIZE: u32 = 69;

const CAPABILITY_SHADER: u32 = 1;
const ADDRESSING_MODEL_LOGICAL: u32 = 0;
const MEMORY_MODEL_GLSL450: u32 = 1;
const EXECUTION_MODEL_VERTEX: u32 = 0;
const EXECUTION_MODEL_FRAGMENT: u32 = 4;
const EXECUTION_MODE_ORIGIN_UPPER_LEFT: u32 = 7;
const DECORATION_BLOCK: u32 = 2;
const DECORATION_BUILTIN: u32 = 11;
const DECORATION_LOCATION: u32 = 30;
const DECORATION_BINDING: u32 = 33;
const DECORATION_DESCRIPTOR_SET: u32 = 34;
const DECORATION_OFFSET: u32 = 35;
const BUILTIN_POSITION: u32 = 0;
const STORAGE_CLASS_UNIFORM_CONSTANT: u32 = 0;
const STORAGE_CLASS_INPUT: u32 = 1;
const STORAGE_CLASS_OUTPUT: u32 = 3;
const STORAGE_CLASS_PUSH_CONSTANT: u32 = 9;
const FUNCTION_CONTROL_NONE: u32 = 0;

// Операнды `OpTypeImage` — не опкоды и не декорации, а числа-перечисления
// формата картинки. У нас всего одна комбинация (обычная 2D-текстура,
// читаемая через сэмплер), поэтому именованы только использованные значения
const DIM_2D: u32 = 1;
const IMAGE_DEPTH_NO: u32 = 0;
const IMAGE_NOT_ARRAYED: u32 = 0;
const IMAGE_SINGLE_SAMPLED: u32 = 0;
/// «1» значит «известно на этапе компиляции шейдера, что эта картинка
/// используется вместе с сэмплером» — то есть будет обёрнута в
/// `OpTypeSampledImage`, а не читаться как storage-изображение
const IMAGE_SAMPLED_WITH_SAMPLER: u32 = 1;
const IMAGE_FORMAT_UNKNOWN: u32 = 0;

// ============================================================================
// Вершинный шейдер Фазы 3: позиция → clip space (Фаза 2, без изменений) плюс
// нормаль → мир → ламбертова яркость, интерполируемая во фрагментный шейдер
// цветом (Гуро). Push-константа теперь несёт СЕМЬ vec4 вместо четырёх: MVP
// (члены 0-3) и матрица нормалей (члены 4-6, см. `NormalMatrixBytes` в
// `pipeline.rs`) — один блок, потому что это ровно то, что кладёт в него
// один вызов `vkCmdPushConstants` за кадр.
//
// **Почему матрица — vec4-столбцы, а не `mat4`.** Та же причина, что в
// Фазе 2 у MVP: `ColMajor`/`MatrixStride` — лишний пласт декораций там, где
// `Offset` одной уже даёт нужный layout.
// ============================================================================

fn vertex_shader() -> Vec<u32> {
    let mut b = SpirvBuilder::new();

    let main_id = b.id();
    let glsl_ext = b.id();
    let void_ty = b.id();
    let voidfn_ty = b.id();
    let float_ty = b.id();
    let v2float_ty = b.id();
    let v3float_ty = b.id();
    let v4float_ty = b.id();
    let in_position_var = b.id();
    let in_normal_var = b.id();
    let in_uv_var = b.id();
    let position_var = b.id();
    let out_color_var = b.id();
    let out_uv_var = b.id();
    // ID структуры push-константы выдаётся здесь, хотя сам `OpTypeStruct`
    // будет записан сильно ниже: её декорации обязаны стоять в секции
    // аннотаций, то есть ДО секции типов (см. блок аннотаций ниже)
    let mvp_struct_ty = b.id();

    b.inst(OP_CAPABILITY, &[CAPABILITY_SHADER]);
    {
        let mut ops = vec![glsl_ext];
        ops.extend(encode_string("GLSL.std.450"));
        b.inst(OP_EXT_INST_IMPORT, &ops);
    }
    b.inst(OP_MEMORY_MODEL, &[ADDRESSING_MODEL_LOGICAL, MEMORY_MODEL_GLSL450]);
    {
        let mut ops = vec![EXECUTION_MODEL_VERTEX, main_id];
        ops.extend(encode_string("main"));
        // Push-constant по-прежнему не входит в интерфейс (SPIR-V 1.0):
        // только Input/Output-переменные
        ops.push(in_position_var);
        ops.push(in_normal_var);
        ops.push(in_uv_var);
        ops.push(position_var);
        ops.push(out_color_var);
        ops.push(out_uv_var);
        b.inst(OP_ENTRY_POINT, &ops);
    }

    b.inst(OP_DECORATE, &[in_position_var, DECORATION_LOCATION, 0]);
    b.inst(OP_DECORATE, &[in_normal_var, DECORATION_LOCATION, 1]);
    b.inst(OP_DECORATE, &[in_uv_var, DECORATION_LOCATION, 2]);
    b.inst(OP_DECORATE, &[position_var, DECORATION_BUILTIN, BUILTIN_POSITION]);
    // Цвет — Location 0 на выходе: свободен, потому что позиция выходит
    // через `BuiltIn`, а не через `Location` (те два пространства номеров
    // не пересекаются). UV — Location 1, рядом
    b.inst(OP_DECORATE, &[out_color_var, DECORATION_LOCATION, 0]);
    b.inst(OP_DECORATE, &[out_uv_var, DECORATION_LOCATION, 1]);

    // Декорации блока push-константы стоят ЗДЕСЬ, вместе со всеми
    // остальными, а не рядом со своим `OpTypeStruct` ниже, — и это не
    // вопрос вкуса. Раскладка модуля SPIR-V (раздел «Logical Layout»)
    // фиксирует порядок секций: аннотации (`OpDecorate`/`OpMemberDecorate`)
    // идут ОДНОЙ группой и строго ПЕРЕД секцией типов, констант и
    // глобальных переменных. Декорация ссылается на ещё не объявленный тип
    // вперёд — это разрешено специально для этого случая (ровно так же
    // выше декорируются переменные, чьи `OpVariable` тоже ниже).
    //
    // Ошибка тут абсолютно тихая на нашей машине: MoltenVK разбирает
    // инструкции подряд и применяет декорации по ID, где бы те ни лежали,
    // поэтому картинка была правильной и с нарушенным порядком. Заметил бы
    // только `spirv-val` или слой валидации — то есть ровно то, чего на
    // машине разработки нет (см. оговорку в doc-комментарии модуля)
    b.inst(OP_DECORATE, &[mvp_struct_ty, DECORATION_BLOCK]);
    for (member, offset) in [(0, 0), (1, 16), (2, 32), (3, 48), (4, 64), (5, 80), (6, 96)] {
        b.inst(OP_MEMBER_DECORATE, &[mvp_struct_ty, member, DECORATION_OFFSET, offset]);
    }

    b.inst(OP_TYPE_VOID, &[void_ty]);
    b.inst(OP_TYPE_FUNCTION, &[voidfn_ty, void_ty]);
    b.inst(OP_TYPE_FLOAT, &[float_ty, 32]);
    b.inst(OP_TYPE_VECTOR, &[v2float_ty, float_ty, 2]);
    b.inst(OP_TYPE_VECTOR, &[v3float_ty, float_ty, 3]);
    b.inst(OP_TYPE_VECTOR, &[v4float_ty, float_ty, 4]);

    let float_1 = b.id();
    b.inst(OP_CONSTANT, &[float_ty, float_1, 1.0f32.to_bits()]);
    let float_0 = b.id();
    b.inst(OP_CONSTANT, &[float_ty, float_0, 0.0f32.to_bits()]);

    // Сама структура push-константы: семь vec4 подряд. Обязательные для
    // любого Uniform/PushConstant блока декорации (`Block` на структуре и
    // `Offset` на каждом члене) записаны выше, в секции аннотаций
    b.inst(
        OP_TYPE_STRUCT,
        &[mvp_struct_ty, v4float_ty, v4float_ty, v4float_ty, v4float_ty, v4float_ty, v4float_ty, v4float_ty],
    );

    let ptr_pushconstant_struct_ty = b.id();
    b.inst(OP_TYPE_POINTER, &[ptr_pushconstant_struct_ty, STORAGE_CLASS_PUSH_CONSTANT, mvp_struct_ty]);
    let mvp_var = b.id();
    // Push-константа не бывает с инициализатором — значение кладёт CPU
    // каждый кадр через `vkCmdPushConstants`, до этого содержимое не
    // определено
    b.inst(OP_VARIABLE, &[ptr_pushconstant_struct_ty, mvp_var, STORAGE_CLASS_PUSH_CONSTANT]);

    let ptr_pushconstant_v4float_ty = b.id();
    b.inst(OP_TYPE_POINTER, &[ptr_pushconstant_v4float_ty, STORAGE_CLASS_PUSH_CONSTANT, v4float_ty]);

    // Индексы членов структуры в OpAccessChain обязаны быть КОНСТАНТАМИ
    // (в отличие от индекса в массиве, который может быть значением
    // переменной) — отсюда семь отдельных int-констант вместо одной
    // переиспользуемой переменной-счётчика
    let int_ty = b.id();
    b.inst(OP_TYPE_INT, &[int_ty, 32, 1]);
    let int_consts: Vec<u32> = (0..7u32)
        .map(|i| {
            let id = b.id();
            b.inst(OP_CONSTANT, &[int_ty, id, i]);
            id
        })
        .collect();

    let ptr_input_v3float_ty = b.id();
    b.inst(OP_TYPE_POINTER, &[ptr_input_v3float_ty, STORAGE_CLASS_INPUT, v3float_ty]);
    b.inst(OP_VARIABLE, &[ptr_input_v3float_ty, in_position_var, STORAGE_CLASS_INPUT]);
    b.inst(OP_VARIABLE, &[ptr_input_v3float_ty, in_normal_var, STORAGE_CLASS_INPUT]);

    let ptr_input_v2float_ty = b.id();
    b.inst(OP_TYPE_POINTER, &[ptr_input_v2float_ty, STORAGE_CLASS_INPUT, v2float_ty]);
    b.inst(OP_VARIABLE, &[ptr_input_v2float_ty, in_uv_var, STORAGE_CLASS_INPUT]);

    let ptr_output_v4float_ty = b.id();
    b.inst(OP_TYPE_POINTER, &[ptr_output_v4float_ty, STORAGE_CLASS_OUTPUT, v4float_ty]);
    b.inst(OP_VARIABLE, &[ptr_output_v4float_ty, position_var, STORAGE_CLASS_OUTPUT]);

    let ptr_output_v3float_ty = b.id();
    b.inst(OP_TYPE_POINTER, &[ptr_output_v3float_ty, STORAGE_CLASS_OUTPUT, v3float_ty]);
    b.inst(OP_VARIABLE, &[ptr_output_v3float_ty, out_color_var, STORAGE_CLASS_OUTPUT]);

    let ptr_output_v2float_ty = b.id();
    b.inst(OP_TYPE_POINTER, &[ptr_output_v2float_ty, STORAGE_CLASS_OUTPUT, v2float_ty]);
    b.inst(OP_VARIABLE, &[ptr_output_v2float_ty, out_uv_var, STORAGE_CLASS_OUTPUT]);

    // Направление на свет — то же значение, что на CPU-пути
    // (`config::LIGHT_DIRECTION`), нормализованное в Rust один раз при
    // сборке модуля, а не в шейдере на каждой вершине: свет не меняется
    // кадр от кадра, пересчитывать одно и то же незачем. `w = 0` — это
    // направление, не точка (см. «Точка против направления» в CLAUDE.md);
    // хранить его в vec4 нужно только затем, что нормаль ниже тоже остаётся
    // в vec4 всю дорогу, а `OpDot` требует одинаковой размерности обоих
    // операндов
    let light_dir = crate::config::LIGHT_DIRECTION.normalize();
    let light_x = b.id();
    b.inst(OP_CONSTANT, &[float_ty, light_x, light_dir.x.to_bits()]);
    let light_y = b.id();
    b.inst(OP_CONSTANT, &[float_ty, light_y, light_dir.y.to_bits()]);
    let light_z = b.id();
    b.inst(OP_CONSTANT, &[float_ty, light_z, light_dir.z.to_bits()]);
    let light_dir_v4 = b.id();
    b.inst(OP_CONSTANT_COMPOSITE, &[v4float_ty, light_dir_v4, light_x, light_y, light_z, float_0]);

    // Те же константы, что `AMBIENT_LIGHT` на CPU-пути — `1 - AMBIENT_LIGHT`
    // считаем в Rust, а не вычитанием в шейдере: одной константой меньше,
    // одним `OpFSub` меньше
    let ambient = b.id();
    b.inst(OP_CONSTANT, &[float_ty, ambient, crate::config::AMBIENT_LIGHT.to_bits()]);
    let one_minus_ambient = b.id();
    b.inst(OP_CONSTANT, &[float_ty, one_minus_ambient, (1.0 - crate::config::AMBIENT_LIGHT).to_bits()]);

    // Базовый цвет грани: материалов на GPU-пути пока нет (текстура из
    // Фазы 4 не отменяет его, а домножается на него во фрагментном шейдере),
    // поэтому это та же оранжевая заглушка, что в Фазе 1 была жёстко зашита
    // во фрагментный шейдер, — просто теперь она умножается на освещение, а
    // не выводится как есть.
    //
    // Красная и синяя компоненты — это `float_1`/`float_0`, объявленные
    // выше, а не свои такие же константы. Дублировать нельзя: неагрегатные
    // типы и константы в SPIR-V уникальны по паре (тип, значение), и два
    // `OpConstant` с одинаковым `float` и одинаковым значением — такая же
    // тихая ошибка раскладки модуля, как порядок секций выше: наш драйвер её
    // проглотит, `spirv-val` — нет
    let base_g = b.id();
    b.inst(OP_CONSTANT, &[float_ty, base_g, 0.55f32.to_bits()]);
    let base_color = b.id();
    b.inst(OP_CONSTANT_COMPOSITE, &[v3float_ty, base_color, float_1, base_g, float_0]);

    b.inst(OP_FUNCTION, &[void_ty, main_id, FUNCTION_CONTROL_NONE, voidfn_ty]);
    let entry_label = b.id();
    b.inst(OP_LABEL, &[entry_label]);

    // Семь столбцов push-константы — по одному access chain на член
    // структуры: первые четыре — MVP, следующие три — матрица нормалей
    let mut cols = Vec::with_capacity(7);
    for &idx in &int_consts {
        let access = b.id();
        b.inst(OP_ACCESS_CHAIN, &[ptr_pushconstant_v4float_ty, access, mvp_var, idx]);
        let col = b.id();
        b.inst(OP_LOAD, &[v4float_ty, col, access]);
        cols.push(col);
    }
    let (mvp_cols, normal_cols) = cols.split_at(4);

    // Позиция вершины → clip space (Фаза 2, без изменений): x, y, z по
    // отдельности — компоненты нужны как скаляры, чтобы «размножить»
    // каждую в vec4 для покомпонентного умножения на столбец
    let position = b.id();
    b.inst(OP_LOAD, &[v3float_ty, position, in_position_var]);
    let px = b.id();
    b.inst(OP_COMPOSITE_EXTRACT, &[float_ty, px, position, 0]);
    let py = b.id();
    b.inst(OP_COMPOSITE_EXTRACT, &[float_ty, py, position, 1]);
    let pz = b.id();
    b.inst(OP_COMPOSITE_EXTRACT, &[float_ty, pz, position, 2]);

    let splat_x = b.id();
    b.inst(OP_COMPOSITE_CONSTRUCT, &[v4float_ty, splat_x, px, px, px, px]);
    let splat_y = b.id();
    b.inst(OP_COMPOSITE_CONSTRUCT, &[v4float_ty, splat_y, py, py, py, py]);
    let splat_z = b.id();
    b.inst(OP_COMPOSITE_CONSTRUCT, &[v4float_ty, splat_z, pz, pz, pz, pz]);
    let splat_w = b.id();
    b.inst(OP_COMPOSITE_CONSTRUCT, &[v4float_ty, splat_w, float_1, float_1, float_1, float_1]);

    let term0 = b.id();
    b.inst(OP_FMUL, &[v4float_ty, term0, mvp_cols[0], splat_x]);
    let term1 = b.id();
    b.inst(OP_FMUL, &[v4float_ty, term1, mvp_cols[1], splat_y]);
    let term2 = b.id();
    b.inst(OP_FMUL, &[v4float_ty, term2, mvp_cols[2], splat_z]);
    let term3 = b.id();
    b.inst(OP_FMUL, &[v4float_ty, term3, mvp_cols[3], splat_w]);

    let sum01 = b.id();
    b.inst(OP_FADD, &[v4float_ty, sum01, term0, term1]);
    let sum012 = b.id();
    b.inst(OP_FADD, &[v4float_ty, sum012, sum01, term2]);
    let clip_position = b.id();
    b.inst(OP_FADD, &[v4float_ty, clip_position, sum012, term3]);
    b.inst(OP_STORE, &[position_var, clip_position]);

    // Нормаль → мир: тот же приём, что для позиции (splat + FMul + FAdd),
    // но по трём столбцам матрицы нормалей вместо четырёх столбцов MVP, и
    // без переноса — у направления нет составляющей w=1 (см. «Точка против
    // направления» в CLAUDE.md), поэтому здесь всего три слагаемых, а не
    // четыре
    let normal = b.id();
    b.inst(OP_LOAD, &[v3float_ty, normal, in_normal_var]);
    let nx = b.id();
    b.inst(OP_COMPOSITE_EXTRACT, &[float_ty, nx, normal, 0]);
    let ny = b.id();
    b.inst(OP_COMPOSITE_EXTRACT, &[float_ty, ny, normal, 1]);
    let nz = b.id();
    b.inst(OP_COMPOSITE_EXTRACT, &[float_ty, nz, normal, 2]);

    let nsplat_x = b.id();
    b.inst(OP_COMPOSITE_CONSTRUCT, &[v4float_ty, nsplat_x, nx, nx, nx, nx]);
    let nsplat_y = b.id();
    b.inst(OP_COMPOSITE_CONSTRUCT, &[v4float_ty, nsplat_y, ny, ny, ny, ny]);
    let nsplat_z = b.id();
    b.inst(OP_COMPOSITE_CONSTRUCT, &[v4float_ty, nsplat_z, nz, nz, nz, nz]);

    let nterm0 = b.id();
    b.inst(OP_FMUL, &[v4float_ty, nterm0, normal_cols[0], nsplat_x]);
    let nterm1 = b.id();
    b.inst(OP_FMUL, &[v4float_ty, nterm1, normal_cols[1], nsplat_y]);
    let nterm2 = b.id();
    b.inst(OP_FMUL, &[v4float_ty, nterm2, normal_cols[2], nsplat_z]);

    let nsum01 = b.id();
    b.inst(OP_FADD, &[v4float_ty, nsum01, nterm0, nterm1]);
    let world_normal = b.id();
    b.inst(OP_FADD, &[v4float_ty, world_normal, nsum01, nterm2]);

    // Нормировка обязательна в общем случае (матрица нормалей — не поворот,
    // если у инстанса неравномерный масштаб, см. CLAUDE.md), и `w = 0` тут
    // не портит длину: `sqrt(x²+y²+z²+0²)` совпадает с трёхмерной длиной.
    // `Normalize` — из GLSL.std.450, не из ядра SPIR-V (см. doc-комментарий
    // модуля — sqrt в самом формате не определён)
    let normalized_normal = b.id();
    b.inst(OP_EXT_INST, &[v4float_ty, normalized_normal, glsl_ext, GLSL_STD_450_NORMALIZE, world_normal]);

    let lambert_raw = b.id();
    b.inst(OP_DOT, &[float_ty, lambert_raw, normalized_normal, light_dir_v4]);
    let lambert = b.id();
    b.inst(OP_EXT_INST, &[float_ty, lambert, glsl_ext, GLSL_STD_450_FMAX, lambert_raw, float_0]);

    let scaled = b.id();
    b.inst(OP_FMUL, &[float_ty, scaled, one_minus_ambient, lambert]);
    let intensity = b.id();
    b.inst(OP_FADD, &[float_ty, intensity, ambient, scaled]);

    let intensity_v3 = b.id();
    b.inst(OP_COMPOSITE_CONSTRUCT, &[v3float_ty, intensity_v3, intensity, intensity, intensity]);
    let out_color = b.id();
    b.inst(OP_FMUL, &[v3float_ty, out_color, base_color, intensity_v3]);
    b.inst(OP_STORE, &[out_color_var, out_color]);

    // UV не участвует ни в MVP, ни в освещении — развёртка задана в
    // плоскости самого атласа, а не в мире, поэтому едет во фрагментный
    // шейдер как есть, без единого умножения
    let uv = b.id();
    b.inst(OP_LOAD, &[v2float_ty, uv, in_uv_var]);
    b.inst(OP_STORE, &[out_uv_var, uv]);

    b.inst(OP_RETURN, &[]);
    b.inst(OP_FUNCTION_END, &[]);

    b.finish()
}

// ============================================================================
// Фрагментный шейдер Фазы 4: тексель из `OpTypeSampledImage` (combined
// image sampler — `descriptor.rs`) умножается на интерполированный цвет
// Гуро (Фаза 3) покомпонентно, тем же приёмом, что `Vec3 * Vec3` на
// CPU-пути, и достраивается до vec4 с alpha = 1 (непрозрачно, как и весь
// остальной движок — см. «альфа текстуры отбрасывается» в CLAUDE.md)
// ============================================================================

fn fragment_shader() -> Vec<u32> {
    let mut b = SpirvBuilder::new();

    let main_id = b.id();
    let void_ty = b.id();
    let voidfn_ty = b.id();
    let float_ty = b.id();
    let v2float_ty = b.id();
    let v3float_ty = b.id();
    let v4float_ty = b.id();
    let in_color_var = b.id();
    let in_uv_var = b.id();
    let tex_var = b.id();
    let out_color_var = b.id();

    b.inst(OP_CAPABILITY, &[CAPABILITY_SHADER]);
    b.inst(OP_MEMORY_MODEL, &[ADDRESSING_MODEL_LOGICAL, MEMORY_MODEL_GLSL450]);
    {
        let mut ops = vec![EXECUTION_MODEL_FRAGMENT, main_id];
        ops.extend(encode_string("main"));
        // Combined image sampler в интерфейс (SPIR-V 1.0) не входит —
        // только Input/Output, как и раньше
        ops.push(in_color_var);
        ops.push(in_uv_var);
        ops.push(out_color_var);
        b.inst(OP_ENTRY_POINT, &ops);
    }
    // Обязателен у КАЖДОГО фрагментного шейдера: говорит, что (0,0) кадра —
    // левый верхний угол (у Vulkan так же, как у растеризатора в этом же
    // движке — см. `v растёт вниз` в CLAUDE.md, ровно та же договорённость)
    b.inst(OP_EXECUTION_MODE, &[main_id, EXECUTION_MODE_ORIGIN_UPPER_LEFT]);

    b.inst(OP_DECORATE, &[in_color_var, DECORATION_LOCATION, 0]);
    b.inst(OP_DECORATE, &[in_uv_var, DECORATION_LOCATION, 1]);
    // Set 0 / Binding 0 — тот самый единственный биндинг, что создаёт
    // `descriptor::create_set_layout`
    b.inst(OP_DECORATE, &[tex_var, DECORATION_DESCRIPTOR_SET, 0]);
    b.inst(OP_DECORATE, &[tex_var, DECORATION_BINDING, 0]);
    b.inst(OP_DECORATE, &[out_color_var, DECORATION_LOCATION, 0]);

    b.inst(OP_TYPE_VOID, &[void_ty]);
    b.inst(OP_TYPE_FUNCTION, &[voidfn_ty, void_ty]);
    b.inst(OP_TYPE_FLOAT, &[float_ty, 32]);
    b.inst(OP_TYPE_VECTOR, &[v2float_ty, float_ty, 2]);
    b.inst(OP_TYPE_VECTOR, &[v3float_ty, float_ty, 3]);
    b.inst(OP_TYPE_VECTOR, &[v4float_ty, float_ty, 4]);

    let float_1 = b.id();
    b.inst(OP_CONSTANT, &[float_ty, float_1, 1.0f32.to_bits()]);

    // `OpTypeImage` описывает саму картинку (2D, без глубины/массива/
    // мультисэмплинга, "известно, что будет читаться через сэмплер"),
    // `OpTypeSampledImage` — картинку и сэмплер вместе, ровно то, во что
    // превращается один `VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER`
    let image_ty = b.id();
    b.inst(
        OP_TYPE_IMAGE,
        &[
            image_ty,
            float_ty,
            DIM_2D,
            IMAGE_DEPTH_NO,
            IMAGE_NOT_ARRAYED,
            IMAGE_SINGLE_SAMPLED,
            IMAGE_SAMPLED_WITH_SAMPLER,
            IMAGE_FORMAT_UNKNOWN,
        ],
    );
    let sampled_image_ty = b.id();
    b.inst(OP_TYPE_SAMPLED_IMAGE, &[sampled_image_ty, image_ty]);

    let ptr_input_v3float_ty = b.id();
    b.inst(OP_TYPE_POINTER, &[ptr_input_v3float_ty, STORAGE_CLASS_INPUT, v3float_ty]);
    b.inst(OP_VARIABLE, &[ptr_input_v3float_ty, in_color_var, STORAGE_CLASS_INPUT]);

    let ptr_input_v2float_ty = b.id();
    b.inst(OP_TYPE_POINTER, &[ptr_input_v2float_ty, STORAGE_CLASS_INPUT, v2float_ty]);
    b.inst(OP_VARIABLE, &[ptr_input_v2float_ty, in_uv_var, STORAGE_CLASS_INPUT]);

    let ptr_uniformconstant_sampledimage_ty = b.id();
    b.inst(
        OP_TYPE_POINTER,
        &[ptr_uniformconstant_sampledimage_ty, STORAGE_CLASS_UNIFORM_CONSTANT, sampled_image_ty],
    );
    b.inst(OP_VARIABLE, &[ptr_uniformconstant_sampledimage_ty, tex_var, STORAGE_CLASS_UNIFORM_CONSTANT]);

    let ptr_output_v4float_ty = b.id();
    b.inst(OP_TYPE_POINTER, &[ptr_output_v4float_ty, STORAGE_CLASS_OUTPUT, v4float_ty]);
    b.inst(OP_VARIABLE, &[ptr_output_v4float_ty, out_color_var, STORAGE_CLASS_OUTPUT]);

    b.inst(OP_FUNCTION, &[void_ty, main_id, FUNCTION_CONTROL_NONE, voidfn_ty]);
    let entry_label = b.id();
    b.inst(OP_LABEL, &[entry_label]);

    let color_rgb = b.id();
    b.inst(OP_LOAD, &[v3float_ty, color_rgb, in_color_var]);
    let uv = b.id();
    b.inst(OP_LOAD, &[v2float_ty, uv, in_uv_var]);

    // Комбинированный image+sampler грузится ОДНИМ `OpLoad` — отдельный
    // `OpSampledImage` нужен только когда картинка и сэмплер приезжают
    // раздельными дескрипторами, а у нас один `COMBINED_IMAGE_SAMPLER`
    // с самого начала
    let sampled_image_val = b.id();
    b.inst(OP_LOAD, &[sampled_image_ty, sampled_image_val, tex_var]);
    let texel = b.id();
    b.inst(OP_IMAGE_SAMPLE_IMPLICIT_LOD, &[v4float_ty, texel, sampled_image_val, uv]);

    // Тексель — vec4, а множим только RGB (альфы у текстуры этого движка
    // нет вовсе, см. doc-комментарий модуля) — три `OpCompositeExtract` и
    // сборка обратно в vec3, тот же приём, что уже применялся к позиции и
    // нормали в вершинном шейдере, вместо нового опкода `OpVectorShuffle`
    let texel_r = b.id();
    b.inst(OP_COMPOSITE_EXTRACT, &[float_ty, texel_r, texel, 0]);
    let texel_g = b.id();
    b.inst(OP_COMPOSITE_EXTRACT, &[float_ty, texel_g, texel, 1]);
    let texel_b = b.id();
    b.inst(OP_COMPOSITE_EXTRACT, &[float_ty, texel_b, texel, 2]);
    let texel_rgb = b.id();
    b.inst(OP_COMPOSITE_CONSTRUCT, &[v3float_ty, texel_rgb, texel_r, texel_g, texel_b]);

    // Покомпонентное произведение — тот же приём, что `Vec3 * Vec3` в
    // `draw_triangle_filled` на CPU-пути: белая текстура обязана оставить
    // освещение как есть
    let final_rgb = b.id();
    b.inst(OP_FMUL, &[v3float_ty, final_rgb, texel_rgb, color_rgb]);

    // Смешивать вектор со скаляром в одном `OpCompositeConstruct` разрешено
    // спецификацией (число компонент операндов обязано лишь В СУММЕ
    // совпасть с результатом) — то же самое тождество, которым в GLSL
    // задан `vec4(rgb, 1.0)`, просто без текстового синтаксиса вокруг
    let final_rgba = b.id();
    b.inst(OP_COMPOSITE_CONSTRUCT, &[v4float_ty, final_rgba, final_rgb, float_1]);
    b.inst(OP_STORE, &[out_color_var, final_rgba]);

    b.inst(OP_RETURN, &[]);
    b.inst(OP_FUNCTION_END, &[]);

    b.finish()
}

// ============================================================================
// Загрузка в VkShaderModule
// ============================================================================

fn create_shader_module(device: &Device, code: &[u32]) -> Result<VkShaderModule, String> {
    let create_info = VkShaderModuleCreateInfo {
        s_type: VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: 0,
        // В БАЙТАХ, не в словах — забыть `* 4` значит объявить модуль
        // вчетверо короче настоящего, и разбор оборвётся посреди инструкции
        code_size: code.len() * 4,
        p_code: code.as_ptr(),
    };
    let mut module = VkShaderModule::NULL;
    let result =
        unsafe { (device.fns.create_shader_module)(device.handle, &create_info, std::ptr::null(), &mut module) };
    if result != VK_SUCCESS {
        return Err(format!("vkCreateShaderModule вернул {result}"));
    }
    Ok(module)
}

pub fn vertex_module(device: &Device) -> Result<VkShaderModule, String> {
    create_shader_module(device, &vertex_shader())
}

pub fn fragment_module(device: &Device) -> Result<VkShaderModule, String> {
    create_shader_module(device, &fragment_shader())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Не проверяет, что шейдер делает то, что задумано (для этого нужен
    /// настоящий GPU — см. `mipmaps_calm_down_the_floor_at_the_horizon` и
    /// подобные интеграционные тесты в основном растеризаторе, здесь
    /// аналога нет и быть не может), но ловит именно тот класс ошибок,
    /// ради которого заведён `SpirvBuilder`: разъехавшийся `word_count`.
    /// Проходит по потоку слов инструкция за инструкцией и требует, чтобы
    /// путь кончился РОВНО на границе модуля — недосчитанное или лишнее
    /// слово в любой инструкции собьёт этот проход на первой же ошибке
    fn validate_word_stream(words: &[u32]) -> Result<(), String> {
        assert_eq!(words[0], 0x0723_0203, "неверный magic number");
        let mut pos = 5; // после заголовка (magic, версия, генератор, bound, reserved)
        while pos < words.len() {
            let header = words[pos];
            let word_count = (header >> 16) as usize;
            if word_count == 0 {
                return Err(format!("нулевой word_count на слове {pos}"));
            }
            pos += word_count;
        }
        if pos != words.len() {
            return Err(format!("поток инструкций не совпал с концом модуля: {pos} != {}", words.len()));
        }
        Ok(())
    }

    #[test]
    fn vertex_shader_word_stream_is_self_consistent() {
        validate_word_stream(&vertex_shader()).unwrap();
    }

    #[test]
    fn fragment_shader_word_stream_is_self_consistent() {
        validate_word_stream(&fragment_shader()).unwrap();
    }

    #[test]
    fn a_truncated_instruction_is_caught_by_the_validator() {
        // Проверяем, что тест выше и правда ловит поломку, а не зелёный
        // просто потому, что ничего толком не проверяет — то же правило,
        // что для остального движка в CLAUDE.md: временно ломаем и смотрим,
        // что стало красным.
        //
        // Отрезать ровно ОДНО последнее слово недостаточно: концовка
        // потока — это `OpStore` (3 слова) `OpReturn` (1 слово)
        // `OpFunctionEnd` (1 слово), и снятие одного-двух слов просто
        // убирает целые однословные инструкции целиком, а по структуре
        // это неотличимо от корректно закончившегося потока (несмотря на
        // то, что модуль семантически сломан — validate_word_stream
        // проверяет только согласованность word_count, а не полноту
        // модуля). Три слова гарантированно режут `OpStore` посередине —
        // ровно тот случай, для которого валидатор и заведён
        let mut words = vertex_shader();
        words.truncate(words.len() - 3);
        assert!(validate_word_stream(&words).is_err());
    }

    #[test]
    fn bound_is_one_past_the_last_issued_id() {
        // OpFunctionEnd не выдаёт ID, значит последний реально выданный —
        // это результат последней инструкции ПЕРЕД ним; проверяем, что
        // bound (слово 3 заголовка) действительно на 1 больше него, а не
        // просто какое-то положительное число
        let mut b = SpirvBuilder::new();
        let a = b.id();
        let c = b.id();
        assert_eq!((a, c), (1, 2));
        let words = b.finish();
        assert_eq!(words[3], 3);
    }
}
