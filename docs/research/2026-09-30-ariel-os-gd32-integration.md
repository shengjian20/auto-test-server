# Ariel OS Porting Research — GD32F470 Evaluation

**Date:** 2026-09-30
**Sources:** ariel-os/ariel-os @ `main` = `b69f1f1b23f31ca855fae193327faae73ead6827` (post-v0.5.0), its embassy fork @ pinned revs, local snapshots `.ref/embassy-net` (0.7.1) / `.ref/embassy-net-driver` (0.2), our repo's `feat/embassy-net` branch.

---

## 1. Ariel OS Architecture: Network Stack Integration

**Ariel OS uses embassy-net directly — no wrapper crate, no custom network stack.** There is no `ariel-os-network`; networking lives inside `src/ariel-os-embassy/`.

### Version matrix

| Crate | Workspace pin | Actually compiled |
|---|---|---|
| embassy-net | `0.9.1` (registry pin) | **ariel-os fork**: `git = "https://github.com/ariel-os/embassy", rev = "37683fe299f1"` (branch `embassy-net-v0.9.1+ariel-os`, backport of embassy-rs/embassy#6195) |
| smoltcp | — | **0.13.1** (0.12.0 also in lock via other deps) |
| embassy-executor | `0.9.1` | registry |
| embassy-time | `0.5.1` | fork rev `3c1b5fbf` |

The fork patches are declared in **`ariel-os-cargo.toml`** under `[patch.crates-io]` (not the root Cargo.toml) — alongside forked `embassy-stm32`, `embassy-nrf`, `embassy-rp`, `embassy-hal-internal`, and the whole esp-hal stack. **Any embassy-side contribution must target these fork branches, not upstream.**

### The network task is the textbook embassy-net shape

`src/ariel-os-embassy/src/net.rs`:
```rust
#[embassy_executor::task]
pub(crate) async fn net_task(mut runner: Runner<'static, NetworkDevice>) -> ! {
    runner.run().await
}
```

Stack creation in `src/ariel-os-embassy/src/lib.rs` (init_task):
```rust
let (stack, runner) = embassy_net::new(
    device,
    config,
    RESOURCES.init_with(StackResources::new),  // CONFIG_NETWORK_MAX_CONCURRENT_SOCKETS = 4
    seed,                                       // RNG or device EUI-48
);
spawner.spawn(net::net_task(runner)).unwrap();
// stack stored in: static STACK: OnceLock<Mutex<CriticalSectionRawMutex, SameExecutorCell<NetworkStack>>>
```

App tasks obtain a `Stack<'static>` copy via `ariel_os::net::network_stack()`. Device selection is pure `cfg_select!` on features: `usb-ethernet` / `wifi` / `ethernet` / `tuntap` / `ltem-nrf-modem`, else `DummyDriver`.

### How official HALs wire MAC drivers: a `NetworkDevice` type alias per backend

- **STM32 built-in MAC** — `src/ariel-os-stm32/src/ethernet.rs`:
  ```rust
  pub type NetworkDevice = Ethernet<'static, ETH, GenericPhy>;
  pub fn device(peripherals: &mut OptionalPeripherals) -> NetworkDevice {
      static PKTS: StaticCell<eth::PacketQueue<4, 4>> = StaticCell::new();
      Ethernet::new(PKTS.init(eth::PacketQueue::<4,4>::new()), /* ETH + Irqs + 8 hardcoded RMII pins */, GenericPhy::new(0), mac_addr)
  }
  ```
  Re-exported via `src/ariel-os-embassy/src/ethernet.rs`: `#[cfg(feature = "ethernet-stm32")] pub(crate) use crate::hal::ethernet::NetworkDevice;`. **RMII pinout is hardcoded** (PA1/PA2/PC1/PA7/PC4/PC5/PG13/PB13/PG11, Nucleo-144 style).
- **External MAC, HAL-agnostic** — `src/ariel-os-embassy/src/ethernet/wiznet.rs`: W5500/W5100S/W6100 over SPI via `embassy-net-wiznet`, its own `Runner` task; works on *any* MCU family, needs only a per-board pin-mapping module (see PR #2238).
- ESP Wi-Fi (`esp-radio`), CYW43, nRF91 LTE, USB CDC-NCM — all funnel into the same single `embassy_net::new()` call.

Docs: `book/src/networking.md` (stack link/config override via `#[ariel_os::config]`), `book/src/ethernet.md`.

---

## 2. Dual-Task smoltcp Sharing — The Official Pattern

**This is the key answer: the official embassy ecosystem pattern is a single `RefCell<Inner>` (iface + SocketSet) shared via `Copy` handles, with only short, never-across-await borrows, sound because all tasks run on one cooperative executor.**

From the pinned fork `embassy-net/src/lib.rs` (v0.9.1; **identical structure in registry 0.7.1** — verified in our local `.ref/embassy-net` snapshot, lines 254–306):

```rust
pub struct Runner<'d, D: Driver> {   // owns the MAC driver
    driver: D,
    stack: Stack<'d>,
}

#[derive(Copy, Clone)]               // Copy! handed out to every task
pub struct Stack<'d> {
    inner: &'d RefCell<Inner>,
}

pub(crate) struct Inner {
    pub(crate) sockets: SocketSet<'static>,   // smoltcp SocketSet, lifetime-erased
    pub(crate) iface: Interface,              // smoltcp Interface
    pub(crate) waker: WakerRegistration,
    ...
}
```

- Creation: `let inner = &*resources.inner.write(RefCell::new(inner));` — the `RefCell` lives inside caller-provided `StackResources` (MaybeUninit slot, not a Mutex).
- Poll task borrows only during each poll iteration:
  ```rust
  pub async fn run(&mut self) -> ! {
      poll_fn(|cx| {
          self.stack.with_mut(|i| i.poll(cx, &mut self.driver));
          Poll::<()>::Pending
      }).await;
      unreachable!()
  }
  ```
- Application tasks do socket ops through `TcpSocket`, whose `TcpIo` calls `stack.with_mut(|i| ...)` — a `borrow_mut()` held for the duration of one non-yielding closure only (`tcp.rs`: every op — `read`, `write`, `connect`, `accept`, even `Drop` which removes the socket handle — is `with_mut` scoped).

**Why this is sound without a mutex:** embassy-executor tasks on one core are cooperative — a `RefCell` borrow is never held across an `.await`, so two tasks can never race; a genuinely overlapping borrow is a panic (fail-fast), not UB. `Stack` is deliberately **not `Send`**.

**Ariel OS adds enforcement on top, none inside:**
- `SameExecutorCell` (`src/ariel-os-embassy/src/cell.rs`, used in `net.rs`): the `Stack` handle can only be taken out by tasks on the *same executor* — compile/link-time guard against cross-executor misuse.
- For its **preemptive OS threads** (distinct from async tasks), `src/ariel-os-embassy/src/delegate.rs` provides `Delegate::lend(&mut T)` / `with(|val| ...)` over signals — an explicit `&mut` lending mechanism instead of raw shared handles.

**Alternative pattern that also exists in the ecosystem** (worth knowing, not what embassy-net does): `embassy-net-driver-channel` — the driver runs in its own task and talks to the net task via RX/TX packet queues, so *driver* state is never shared. But the *stack* (Interface/SocketSet) is still RefCell-shared. The device-task-owns-everything + channels pattern applies at the driver boundary, not the socket boundary.

---

## 3. Board Definition Requirements (Minimal Bring-Up Checklist)

Official guide: `book/src/adding-board-support.md`. Three levels:

**(a) Board** — one SBD YAML in `boards/`, e.g. `boards/st-nucleo-f401re.yaml`:
```yaml
version: 0.4.1
targets:
  st-nucleo-f401re:
    chip: stm32f401re        # must match a laze context
    ariel:
      swi: USART2            # STM32 lacks a true SWI: pick any unused IRQ for the executor
    leds:    [{ pin: PA5 }]
    buttons: [{ pin: PC13 }]
```
Then regenerate: `sbd-gen generate-ariel boards -o src/ariel-os-boards --mode update` (generates the `ariel-os-boards` crate: one `.rs` per board, dispatched by `cfg_if!` on `context = "<board>"`). Add a `doc/support_matrix.yml` entry.

**(b) Chip (MCU)** — in `laze-project.yml`:
```yaml
- name: stm32f401re
  parent: stm32
  selects:
    - cortex-m4f             # -> RUSTC_TARGET thumbv7em-none-eabihf (laze-project.yml L911-918)
  env:
    PROBE_RS_CHIP: STM32F401RE
    RUSTFLAGS: [--cfg capability="async-flash-driver", ...]
```
Plus `boards/ariel-chips.yaml` entry and an RCC/clock `default()` in the family crate (`src/ariel-os-stm32/src/rcc.rs`).

**(c) HAL family** — `ariel-os-hal` is **not a trait**, it's a cfg-dispatch facade (`src/ariel-os-hal/src/hal.rs`):
```rust
cfg_select! {
    context = "nrf"   => { pub use ariel_os_nrf::*; }
    context = "rp"    => { pub use ariel_os_rp::*; }
    context = "esp"   => { pub use ariel_os_esp::*; }
    context = "stm32" => { pub use ariel_os_stm32::*; }
    ...
}
```
A new family = new crate (template: copy `ariel-os-stm32`), one `cfg_select!` arm here, plus touchpoints in `ariel-os-storage` / `ariel-os-embassy`.

**Concrete PRs to copy:** #2238 (Waveshare ESP32-S3-ETH — board + W5500 pin mapping; closest analog for "new board + Ethernet"), #2257 (Xiao ESP32-S3), #1002 (NUCLEO-F411RE, plain board on existing family).

### Minimal GD32F470 bring-up checklist (derived)
1. `boards/ariel-chips.yaml`: add `gd32f470`.
2. `laze-project.yml`: context `gd32f470` → `selects: [cortex-m4f]` (gives `thumbv7em-none-eabihf`), `PROBE_RS_CHIP: GD32F470...`.
3. New crate `src/ariel-os-gd32/` implementing the ariel-os-hal surface: `OptionalPeripherals`, init, RNG hook, time driver.
4. `src/ariel-os-hal/src/hal.rs`: add `context = "gd32"` arm; add dep in `src/ariel-os-hal/Cargo.toml`.
5. Time driver: implement `embassy-time-driver` (our 1 MHz TICK timer_driver is a direct candidate).
6. Ethernet: `NetworkDevice` implementing `embassy-net-driver::Driver` (our `EnetDriver` already does exactly this).
7. `boards/<board>.yaml` + `sbd-gen` regen + `support_matrix.yml`.

---

## 4. GD32 Status in Ariel OS

**Zero.** GitHub search API (`q=gd32 org:ariel-os`): 0 issues, 0 PRs, 0 code hits, no fork repos. Repo-wide grep for `gd32`: nothing. Embassy upstream has no GD32 support either, so the ariel-os forks have none.

**Supported families (exactly 4 + native/std):**

| Family | Crate | HAL base |
|---|---|---|
| STM32 | `ariel-os-stm32` + `-mapping` | embassy-stm32 **fork** |
| nRF | `ariel-os-nrf` | embassy-nrf fork |
| RP2040/235x | `ariel-os-rp` | embassy-rp |
| ESP32 | `ariel-os-esp` | esp-hal (forked!) |

Good news: `cortex-m4f` (thumbv7em-none-eabihf) is a first-class laze module — the FPU-enabled startup path exists (`src/ariel-os-rt/src/cortexm.rs` enables FPU for `armv7m_eabihf`). Per-chip feature status tracked in `doc/support_matrix.yml`.

**Blocking dependency:** a GD32 port requires a whole new HAL family (there is no embassy-gd32 to lean on anywhere in their tree) — i.e., our `embassy-gd32` would have to be reshaped into `ariel-os-gd32` implementing their HAL surface, or GD32 registers smuggled through an stm32-family context (register-map compatible in places, but upstream-hostile and fragile).

---

## 5. Minimum Requirements

From root `Cargo.toml` @ `b69f1f1b`:

| Requirement | Value |
|---|---|
| Rust edition / MSRV | **edition 2024, rust-version 1.95** (stable; `criticalup.toml` shows optional Ferrocene qualification) |
| Executor | **embassy-executor 0.9.1**, one of two modes, compile-error-enforced: `executor-interrupt` (SWI-driven; STM32 boards must name the SWI IRQ in board YAML) or `executor-thread` (embassy executor hosted inside an Ariel OS preemptive thread) |
| Allocator | `ariel-os-alloc` → **`embedded_alloc::TlsfHeap`** global allocator on Cortex-M (not loligo / not ia-supercontext / not embassy-memory); heap size via `CONFIG_HEAPSIZE` (default 2048 B, validated against `__sheap`/`__eheap` linker symbols) |
| Time | **embassy-time 0.5.1**; the HAL must supply the driver (`time = ["embassy-stm32/time-driver-any"]`); `net` feature hard-depends on `ariel-os-hal/time`; `embassy-time-driver 0.2.2`, `embassy-time-queue-utils 0.3.0` (generic queue forced when threading) |
| Core | cortex-m 0.7 (inline-asm), cortex-m-rt 0.7.5, portable-atomic `require-cas` (fine on M4F), critical-section 1.2 |
| Extras | OS threads = custom preemptive scheduler (`ariel-os-threads`); storage, random, logging modules each with their own HAL touchpoints |

---

## 6. Recommended Integration Path for This Repo

### Verdict: adopt the embassy-net internal pattern — which `feat/embassy-net` has ALREADY done. Do not adopt Ariel OS.

**The stated blocker ("Runner::run poll_fn vs main task both touch the same Interface/SocketSet without synchronization") is already solved in the current WIP branch.** Evidence from `git show feat/embassy-net:src/firmware/control-server/src/bin/control_server_tcp.rs`:

- `embassy_net::new(EnetDriver{...}, ...)` → `(stack, runner)` split — exactly the upstream design.
- `net_task(runner)` owns the `Runner`; `runner.run()` borrows `Inner` only inside each `poll_fn` iteration.
- The main task holds `stack` (`Stack<'d>` = `Copy` over `&RefCell<Inner>`) and does all socket work through `TcpSocket::new(stack, ...)` → `TcpIo::with_mut` short borrows.
- Synchronization **is** the `RefCell` — the same `&RefCell<Inner>` is the official mechanism. Soundness comes from the single cooperative executor (`executor-thread` mode in our Cargo.toml): borrows never span an `.await`, conflicts panic instead of racing.
- The hard part we already validated on-board is the *driver* contract: `EnetDriver` registers a waker when `receive`/`transmit` return `None` (the "Runner::run 永挂" fix, commit `5218bed`), plus the 2ms `poll_kicker` as the external-kick for a non-interrupt-driven MAC, and TBU/RBU suspend recovery.

**Residual gaps in our branch (small, no architecture change needed):**
1. `link_state()` is a stub (恒 Up under LBM) — wire the PHY status in; the borrow conflict noted in comments dissolves once PHY access goes through its own register handle or an `AtomicBool` updated by a PHY task.
2. `poll_kicker` polls every 2ms — later, ENET DMA IRQ → waker registration replaces the periodic kick (same `net_poll::WAKER` critical-section mutex already in place).
3. Version skew: branch uses registry `embassy-net 0.7` + `smoltcp 0.11`; Ariel OS uses forked `embassy-net 0.9.1` + `smoltcp 0.13.1`. The sharing architecture is byte-for-byte the same across both versions, so no urgency; track upstream only for API changes we care about.

### Why not Ariel OS (cost/benefit)
- Ariel OS adds **zero** smoltcp-sharing technology we don't already have — it consumes embassy-net exactly as we do (`net_task(runner.run())`, Copy Stack handles, `SameExecutorCell` wrapper being the only extra, and we're single-executor so it's moot).
- Adoption cost: MSRV 1.95 / edition 2024; a full `ariel-os-gd32` HAL family (nobody has done GD32 — zero prior art in the org); rewriting our validated gpio/spi/usart/can/enet/timer stack into their HAL surface; forked-embassy rev pinning means any GD32 HAL work lands in *their* fork branches, not upstream; plus threading/storage/random modules we don't need. Our 13/13 board-validated E2E stack would be a rewrite casualty.
- Ariel OS's genuinely attractive pieces (preemptive threads via `Delegate` lending, storage, Ferrocene qualification) solve problems this project doesn't currently have.

### If multi-thread/multi-executor socket access ever becomes a requirement
Steal two patterns from Ariel OS rather than adopting it:
- `SameExecutorCell` (src/ariel-os-embassy/src/cell.rs) — compile-time guarantee the `Stack` handle stays on one executor.
- `Delegate` lend/with (src/ariel-os-embassy/src/delegate.rs) — `&mut` lending across preemptive threads.
- Or invert ownership at the *driver* boundary with `embassy-net-driver-channel` (driver task + RX/TX queues), keeping the stack RefCell untouched.

### Action list
1. Merge-harden `feat/embassy-net` as-is: the architecture is upstream-canonical. Fix `link_state` (PHY status), keep the waker contract fix.
2. Optional: swap 2ms kicker for ENET IRQ-driven wake (`net_poll::WAKER` already exists).
3. File a note against future embassy-net upgrades: our HAL only depends on `embassy-net-driver` (0.2, trait-stable by design), so `embassy-net` 0.7→0.9 upgrades touch only the firmware bin, not the HAL.
4. Re-evaluate Ariel OS only if the project needs preemptive OS threads or certified toolchains — at that point, budget a full `ariel-os-gd32` family port as the entry fee.
