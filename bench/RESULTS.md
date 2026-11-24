# Bobba vs G-Earth

Measured on 2026-11-24 on an Apple M5 Pro (15 cores), macOS 26.5.1, against G-Earth 1.5.4-beta-29 (the macOS aarch64 release and its bundled JRE 21). Both sides use the same input files and the same timing method: warm-up rounds, then the median of the measured rounds.

## Proxy, end to end

One load generator (`bench_load`) talks to an echo hotel (`bench_echo`) directly, then through each proxy. Each proxy runs its real pipeline:

- **Bobba:** `DirectProxy` with the real `ExtensionHost` (`bench_bobba_proxy`).
- **G-Earth:** `FlashProxyProvider.startProxyThread` with a real `HConnection` and `ExtensionHandler`, including G-Earth's internal extensions (`gearth/ProxyBench.java`).

No extensions are connected. Round trips are 10,000 sequential chat-sized packets; the flood is 200,000 of the same packet written at once and echoed back, so each packet crosses the proxy twice.

| | RTT p50 | RTT p99 | Flood |
|---|---|---|---|
| Direct, no proxy | 18 µs | 34 µs | — |
| G-Earth | 38 µs | 52–56 µs | 150k–290k packets/s |
| Bobba | 40 µs | 59–65 µs | 2.1M–2.3M packets/s |

Bobba moves 7–14× more packets per second. Its typical latency is about 2.5 µs higher: each packet hops from the reader task to the writer task, where G-Earth writes on the thread that read it.

| Memory after three runs | Physical footprint |
|---|---|
| G-Earth (JVM) | 86 MB |
| Bobba | 16.6 MB (2.5 MB RSS idle) |

## Key recovery

The input is a 280-byte encrypted login stream and nine scraped S-boxes: eight decoys plus, in the success case, the client's real table. G-Earth runs `Rc4Obtainer`'s brute force, copied line for line, with G-Earth's own `RC4` and `FlashBuffer` classes.

| | G-Earth | Bobba |
|---|---|---|
| Key present | 2.5 ms | 1.0 ms |
| No key, giving up | 450 ms | 0.44 ms |
| Picked the right key on this input | **no** | yes (2 candidates, settled on probation) |

G-Earth's slow path tries every `i` for every table. Rewinding n bytes always ends at `i = start − n`, so only `i = n mod 256` can come out fresh, and the slow path cannot succeed. It only delays failure, here by three orders of magnitude.

The wrong pick is not a fluke of this input. `bench_ambiguity` runs 1,000 handshakes with random keys over the same login stream:

| | Count |
|---|---|
| More than one state decrypts into whole frames | 53 |
| G-Earth's first match is the wrong key | 23 (about one login in 43) |
| Bobba picks the wrong key (probation on one following packet) | 0 |

A wrong key decrypts the rest of the session into garbage. Bobba's live test against game-br also produced three candidate states.

## Expressions

10,000 random packets (ints, strings, booleans; header ids below 4000):

| | G-Earth | Bobba |
|---|---|---|
| Parse expression into a packet | 25.2 ms (2.5 µs each) | 4.5 ms (0.45 µs each) |
| Predict the structure and format the expression | 13.9 ms (1.4 µs each) | 6.2 ms (0.62 µs each) |

## Reproduce

```sh
cargo build --release -p bobba-bench
./target/release/bench_inputs /tmp/bobba-bench/inputs
./target/release/bench_micro /tmp/bobba-bench/inputs
./target/release/bench_ambiguity

G=/path/to/G-Earth.app/Contents/Resources/Java
JRE=/path/to/G-Earth.app/Contents/PlugIns/jre.jre/Contents/Home
CP="$G/G-Earth-runnable.jar:$(ls $G/libs/*.jar | tr '\n' ':')"
javac --system "$JRE" --add-modules javafx.graphics -cp "$CP" -d /tmp/bobba-bench/classes bench/gearth/*.java
$JRE/bin/java -cp "/tmp/bobba-bench/classes:$CP" Bench /tmp/bobba-bench/inputs

./target/release/bench_echo 31000 &
./target/release/bench_bobba_proxy 31002 31000 &
$JRE/bin/java -cp "/tmp/bobba-bench/classes:$CP" ProxyBench 31001 31000 &
./target/release/bench_load 31000 direct
./target/release/bench_load 31002 bobba
./target/release/bench_load 31001 gearth
```

`javac` comes from any JDK 21; G-Earth's bundled JRE has no compiler, so `--system` points the compile at it for JavaFX.
