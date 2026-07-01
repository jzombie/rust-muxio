While there is not a single, ubiquitous "plug-and-play" crate (like `tokio` or `quinn`) for Deficit Round-Robin (DRR) in the standard Rust ecosystem, the research notes provide several specific GitHub repositories and architectural references that implement this exact pattern:

**1. HelixRouter (`Mattbusel/HelixRouter-adaptive-async-compute-router`)**
The notes directly cite this repository as a primary reference implementation for building adaptive, async DRR schedulers in Rust. It serves as a blueprint for implementing strict bandwidth fairness across multiple independent stream queues without relying on shared channel bottlenecks.

**2. Fila (`faiscadev/fila`)**
This is cited as an advanced Rust message broker where fair scheduling and per-key throttling are treated as first-class primitives. It demonstrates how to apply fair-queuing mechanics at the application layer to prevent single tenants or streams from starving a shared system.

**3. PANDEMONIUM & scx_cake (`wllclngn/PANDEMONIUM`)**
Outside of just RPC frameworks, the notes also point to Rust-based Linux CPU schedulers (built using the eBPF `sched_ext` framework). These projects utilize DRR to dynamically learn task behaviors and schedule CPU time with absolute $O(1)$ fairness.

**How it is actually built in these systems:**
The notes emphasize that if you are building this yourself, you should **not** just spin up 1,000 standard `tokio::sync::mpsc::unbounded_channel` instances to act as your isolated stream queues. The memory overhead and state-machine bloat of that many Tokio channels would cripple the CPU cache. 

Instead, the reference implementations typically construct these isolated queues using **intrusive linked lists** or **highly optimized `Vec` chunked arrays**. A single, dedicated background dispatcher task then rapidly loops over these lightweight arrays, adding quantums to their deficit counters and draining the data strictly according to the DRR rules we discussed.
