// Wrangler imports a `.wasm` file as a compiled module, ready to instantiate.
declare module "*.wasm" {
  const module: WebAssembly.Module;
  export default module;
}
