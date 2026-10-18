// generated from observability/catalog/categories.json; run node observability/catalog/generate.mjs.
import type { Category, CategoryDescriptor } from "./types.js";

export const catalogVersion: number = 1;

export const categories: Readonly<{
  readonly engine: Category;
  readonly engineCpu: Category;
  readonly engineCpuCache: Category;
  readonly engineCpuCodegen: Category;
  readonly engineCpuDecode: Category;
  readonly engineLoader: Category;
  readonly engineLoaderLinker: Category;
  readonly engineLoaderModules: Category;
  readonly engineLoaderPe32: Category;
  readonly host: Category;
  readonly hostBrowser: Category;
  readonly hostFiles: Category;
  readonly hostGraphics: Category;
  readonly hostWasm: Category;
  readonly hostWorker: Category;
  readonly observability: Category;
}> = Object.freeze({
  engine: "engine" as Category,
  engineCpu: "engine.cpu" as Category,
  engineCpuCache: "engine.cpu.cache" as Category,
  engineCpuCodegen: "engine.cpu.codegen" as Category,
  engineCpuDecode: "engine.cpu.decode" as Category,
  engineLoader: "engine.loader" as Category,
  engineLoaderLinker: "engine.loader.linker" as Category,
  engineLoaderModules: "engine.loader.modules" as Category,
  engineLoaderPe32: "engine.loader.pe32" as Category,
  host: "host" as Category,
  hostBrowser: "host.browser" as Category,
  hostFiles: "host.files" as Category,
  hostGraphics: "host.graphics" as Category,
  hostWasm: "host.wasm" as Category,
  hostWorker: "host.worker" as Category,
  observability: "observability" as Category,
});

export const categoryCatalog: readonly CategoryDescriptor[] = Object.freeze([
  Object.freeze({
    id: categories.engine,
    label: "Engine",
    description: "Guest execution and Windows compatibility runtime.",
    parent: null,
    selectable: true,
  }),
  Object.freeze({
    id: categories.engineCpu,
    label: "CPU and translation",
    description: "Guest CPU execution, translation and code cache activity.",
    parent: categories.engine,
    selectable: true,
  }),
  Object.freeze({
    id: categories.engineCpuCache,
    label: "Code cache",
    description: "Admitting, finding and removing translated code.",
    parent: categories.engineCpu,
    selectable: true,
  }),
  Object.freeze({
    id: categories.engineCpuCodegen,
    label: "Code generation",
    description: "Translating decoded guest code into WebAssembly.",
    parent: categories.engineCpu,
    selectable: true,
  }),
  Object.freeze({
    id: categories.engineCpuDecode,
    label: "Instruction decoding",
    description: "Decoding x86 instructions and forming translation inputs.",
    parent: categories.engineCpu,
    selectable: true,
  }),
  Object.freeze({
    id: categories.engineLoader,
    label: "Executable loading",
    description: "Loading executable images and resolving their dependencies.",
    parent: categories.engine,
    selectable: true,
  }),
  Object.freeze({
    id: categories.engineLoaderLinker,
    label: "Linking",
    description: "Resolving imports, exports and image relocations.",
    parent: categories.engineLoader,
    selectable: true,
  }),
  Object.freeze({
    id: categories.engineLoaderModules,
    label: "Module dependencies",
    description: "Requesting and supplying guest executable dependencies.",
    parent: categories.engineLoader,
    selectable: true,
  }),
  Object.freeze({
    id: categories.engineLoaderPe32,
    label: "PE32 images",
    description: "Parsing and mapping Windows PE32 executable images.",
    parent: categories.engineLoader,
    selectable: true,
  }),
  Object.freeze({
    id: categories.host,
    label: "Host",
    description: "Browser APIs and the host runtime driving the engine.",
    parent: null,
    selectable: true,
  }),
  Object.freeze({
    id: categories.hostBrowser,
    label: "Browser lifecycle",
    description: "Starting, controlling and completing browser game sessions.",
    parent: categories.host,
    selectable: true,
  }),
  Object.freeze({
    id: categories.hostFiles,
    label: "Host file access",
    description: "Providing executable and game data bytes to the engine.",
    parent: categories.host,
    selectable: true,
  }),
  Object.freeze({
    id: categories.hostGraphics,
    label: "Host graphics",
    description: "Preparing browser graphics resources and presenting output.",
    parent: categories.host,
    selectable: true,
  }),
  Object.freeze({
    id: categories.hostWasm,
    label: "WebAssembly host",
    description: "Compiling, instantiating and installing WebAssembly code.",
    parent: categories.host,
    selectable: true,
  }),
  Object.freeze({
    id: categories.hostWorker,
    label: "Runtime worker",
    description: "Worker execution, scheduling and guest service dispatch.",
    parent: categories.host,
    selectable: true,
  }),
  Object.freeze({
    id: categories.observability,
    label: "Recording control",
    description: "Recording lifecycle, completeness and diagnostic failures.",
    parent: null,
    selectable: false,
  }),
]);
