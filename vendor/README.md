# vendor

`candle-core/` は crates.io の candle-core 0.11.0 そのもので、`Cargo.toml` の cudarc の機能だけを
`dynamic-linking` から `dynamic-loading` に替えてある。CUDA 版の kikitori が、NVIDIA のドライバと
CUDA のランタイムの無い PC でも起動し、GPU が使えないときは CPU に戻れるようにするため
（`dynamic-linking` だと起動の時点で `libcuda.so.1` が要る）。ルートの `Cargo.toml` の `[patch.crates-io]` で使う。
candle を上げるときは、同じ版のソースで置き換えて、同じ 1 行を替える。

`[patch.crates-io]` はこのリポジトリでビルドするときだけ効く。crates.io から `kikitori` を `cuda` の機能付きで入れると、
candle-core は crates.io のもの（`dynamic-linking`）になり、起動に `libcuda.so.1` が要る。
