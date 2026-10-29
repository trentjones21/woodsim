// Plans drawn up by the game's own Rust (crates/plans-wasm), compiled to WebAssembly.

import plansModule from "../build/plans.wasm";

/** What the plans module exports; crates/plans-wasm says what each does. */
interface Exports {
  memory: WebAssembly.Memory;
  alloc(len: number): number;
  draw_up_at(at: number, len: number): number;
  output(which: number): number;
  output_len(which: number): number;
  recipe_version(): number;
}

export interface Drawn {
  /** The plan page, as the game would save it. */
  page: string;
  /** The recipe written again as this woodsim writes it, its version line first. */
  recipe: string;
  /** Which way of writing wood `recipe` is in. */
  recipeVersion: number;
}

const decoder = new TextDecoder();

/**
 * Draws up plans for a recipe's text, or says why it can't: the recipe doesn't read, or
 * drawing it up went wrong. Each recipe gets an instance of its own, so nothing carries over
 * from one to the next and a recipe that panics only traps its own.
 */
export function drawUp(recipe: Uint8Array): Drawn | { why: string } {
  const plans = exportsOf(new WebAssembly.Instance(plansModule, {}));
  const at = plans.alloc(recipe.length);
  new Uint8Array(plans.memory.buffer, at, recipe.length).set(recipe);
  let status: number;
  try {
    status = plans.draw_up_at(at, recipe.length);
  } catch (error) {
    console.error(JSON.stringify({ message: "drawing up plans trapped", error: String(error) }));
    return { why: "drawing up its plans went wrong" };
  }
  // Read memory afresh: drawing up may have grown it, which leaves an old view of it empty.
  const output = (which: number) =>
    decoder.decode(
      new Uint8Array(plans.memory.buffer, plans.output(which), plans.output_len(which)),
    );
  if (status !== 0) {
    return { why: output(0) };
  }
  return { page: output(0), recipe: output(1), recipeVersion: plans.recipe_version() };
}

/** The plans module's exports, checked to be the ones this Worker was written for. */
function exportsOf(instance: WebAssembly.Instance): Exports {
  const { memory, alloc, draw_up_at, output, output_len, recipe_version } = instance.exports;
  if (
    !(memory instanceof WebAssembly.Memory) ||
    typeof alloc !== "function" ||
    typeof draw_up_at !== "function" ||
    typeof output !== "function" ||
    typeof output_len !== "function" ||
    typeof recipe_version !== "function"
  ) {
    throw new Error("the plans module doesn't export what this Worker expects");
  }
  return {
    memory,
    alloc: alloc as Exports["alloc"],
    draw_up_at: draw_up_at as Exports["draw_up_at"],
    output: output as Exports["output"],
    output_len: output_len as Exports["output_len"],
    recipe_version: recipe_version as Exports["recipe_version"],
  };
}
