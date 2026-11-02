# Persistent tree

Edit a node and compare two versions. Rust creates a new root and copies only
the path to the edited node. Every other subtree is shared with the previous
version. Editing a leaf creates 4 objects and shares the other 11.

The two trees show the previous and current versions. Matching `#` labels mean
the same GC object, compared by reference identity in JavaScript. Green nodes
are shared; orange nodes are new in the current version.

JavaScript keeps only the previous and current GC roots. Each edit moves the
current version to the left, puts the new version on the right, and releases the
oldest version. Nodes shared by either remaining version stay alive. The object
count shows nodes referenced by these two versions, not heap usage.

The discarded-node list observes objects through `WeakRef`. It marks a node as
collected only after its weak reference clears, retaining just its number and
value for display.

From the repository root, with Node.js and the Rust Wasm target installed:

```sh
rustup target add wasm32-unknown-unknown
node examples/tree/web/serve.mjs
```

Open <http://localhost:4173/examples/tree/>. Click a node in the current tree to
edit its value directly. Press Enter to save or Escape to cancel.

After the second edit, nodes unique to the oldest version appear as
**Waiting for GC**, then **Collected** when the browser reclaims them.

Build static files without starting the server:

```sh
node examples/tree/web/build.mjs
```

The output is `target/site/examples/tree/`. To preview the guide alongside it,
run `mdbook build guide --dest-dir target/site` before building examples.

