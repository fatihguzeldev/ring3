// generated from observability/catalog/categories.json; run node observability/catalog/generate.mjs.
use super::{Category, CategoryDescriptor};

pub const CATALOG_VERSION: u32 = 1;
pub(super) const CATALOG_SOURCE_FINGERPRINT: u64 = 0x3b05e987b5eb496b;

pub mod categories {
    use super::Category;
    pub const ENGINE: Category = Category::new_unchecked("engine");
    pub const ENGINE_CPU: Category = Category::new_unchecked("engine.cpu");
    pub const ENGINE_CPU_CACHE: Category = Category::new_unchecked("engine.cpu.cache");
    pub const ENGINE_CPU_CODEGEN: Category = Category::new_unchecked("engine.cpu.codegen");
    pub const ENGINE_CPU_DECODE: Category = Category::new_unchecked("engine.cpu.decode");
    pub const ENGINE_LOADER: Category = Category::new_unchecked("engine.loader");
    pub const ENGINE_LOADER_LINKER: Category = Category::new_unchecked("engine.loader.linker");
    pub const ENGINE_LOADER_MODULES: Category = Category::new_unchecked("engine.loader.modules");
    pub const ENGINE_LOADER_PE32: Category = Category::new_unchecked("engine.loader.pe32");
    pub const HOST: Category = Category::new_unchecked("host");
    pub const HOST_BROWSER: Category = Category::new_unchecked("host.browser");
    pub const HOST_FILES: Category = Category::new_unchecked("host.files");
    pub const HOST_GRAPHICS: Category = Category::new_unchecked("host.graphics");
    pub const HOST_WASM: Category = Category::new_unchecked("host.wasm");
    pub const HOST_WORKER: Category = Category::new_unchecked("host.worker");
    pub const OBSERVABILITY: Category = Category::new_unchecked("observability");
}

pub const CATEGORIES: &[CategoryDescriptor] = &[
    CategoryDescriptor {
        id: "engine",
        key: "engine",
        label: "Engine",
        description: "Guest execution and Windows compatibility runtime.",
        parent: None,
        selectable: true,
    },
    CategoryDescriptor {
        id: "engine.cpu",
        key: "engineCpu",
        label: "CPU and translation",
        description: "Guest CPU execution, translation and code cache activity.",
        parent: Some("engine"),
        selectable: true,
    },
    CategoryDescriptor {
        id: "engine.cpu.cache",
        key: "engineCpuCache",
        label: "Code cache",
        description: "Admitting, finding and removing translated code.",
        parent: Some("engine.cpu"),
        selectable: true,
    },
    CategoryDescriptor {
        id: "engine.cpu.codegen",
        key: "engineCpuCodegen",
        label: "Code generation",
        description: "Translating decoded guest code into WebAssembly.",
        parent: Some("engine.cpu"),
        selectable: true,
    },
    CategoryDescriptor {
        id: "engine.cpu.decode",
        key: "engineCpuDecode",
        label: "Instruction decoding",
        description: "Decoding x86 instructions and forming translation inputs.",
        parent: Some("engine.cpu"),
        selectable: true,
    },
    CategoryDescriptor {
        id: "engine.loader",
        key: "engineLoader",
        label: "Executable loading",
        description: "Loading executable images and resolving their dependencies.",
        parent: Some("engine"),
        selectable: true,
    },
    CategoryDescriptor {
        id: "engine.loader.linker",
        key: "engineLoaderLinker",
        label: "Linking",
        description: "Resolving imports, exports and image relocations.",
        parent: Some("engine.loader"),
        selectable: true,
    },
    CategoryDescriptor {
        id: "engine.loader.modules",
        key: "engineLoaderModules",
        label: "Module dependencies",
        description: "Requesting and supplying guest executable dependencies.",
        parent: Some("engine.loader"),
        selectable: true,
    },
    CategoryDescriptor {
        id: "engine.loader.pe32",
        key: "engineLoaderPe32",
        label: "PE32 images",
        description: "Parsing and mapping Windows PE32 executable images.",
        parent: Some("engine.loader"),
        selectable: true,
    },
    CategoryDescriptor {
        id: "host",
        key: "host",
        label: "Host",
        description: "Browser APIs and the host runtime driving the engine.",
        parent: None,
        selectable: true,
    },
    CategoryDescriptor {
        id: "host.browser",
        key: "hostBrowser",
        label: "Browser lifecycle",
        description: "Starting, controlling and completing browser game sessions.",
        parent: Some("host"),
        selectable: true,
    },
    CategoryDescriptor {
        id: "host.files",
        key: "hostFiles",
        label: "Host file access",
        description: "Providing executable and game data bytes to the engine.",
        parent: Some("host"),
        selectable: true,
    },
    CategoryDescriptor {
        id: "host.graphics",
        key: "hostGraphics",
        label: "Host graphics",
        description: "Preparing browser graphics resources and presenting output.",
        parent: Some("host"),
        selectable: true,
    },
    CategoryDescriptor {
        id: "host.wasm",
        key: "hostWasm",
        label: "WebAssembly host",
        description: "Compiling, instantiating and installing WebAssembly code.",
        parent: Some("host"),
        selectable: true,
    },
    CategoryDescriptor {
        id: "host.worker",
        key: "hostWorker",
        label: "Runtime worker",
        description: "Worker execution, scheduling and guest service dispatch.",
        parent: Some("host"),
        selectable: true,
    },
    CategoryDescriptor {
        id: "observability",
        key: "observability",
        label: "Recording control",
        description: "Recording lifecycle, completeness and diagnostic failures.",
        parent: None,
        selectable: false,
    },
];
