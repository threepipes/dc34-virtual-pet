# dc34-virtual-pet

DEF CON 34 のバッジ（Baochip-1x / baosec）上で稼働する、ペット育成シミュレーション。

設計の軸は 2 点。どちらもこの端末の性質から来ている。

- **累計稼働時間で進む** — 壁時計時刻は使わない
- **電源が頻繁に落ちる前提** — 電源断を第一級の状態として扱う

## ドキュメント

| ファイル | 内容 |
|---|---|
| [docs/spec.md](docs/spec.md) | ゲーム仕様（決定事項）と設計方針 |
| [docs/UI.md](docs/UI.md) | 画面デザイン・遷移図・ボタン割り当て |

## 構成

```
dc34-virtual-pet/
├── core/     -- Xous 非依存のゲームロジック。cargo test が普通に回る
└── src/      -- Xous 依存の描画・入力アダプタ。BadgeGame trait を実装
```

ゲームロジックを Xous から切り離しておくことで、
「72 時間放置したら死ぬ」といった長時間シナリオを `cargo test` で一瞬で検証できる。

```bash
cd core && cargo test
```

`core` は依存クレートを 1 つも持たないので、Xous のツールチェインが無くても回る。

## 組み込み方

このクレートは単体では動かない。`dc34-vault` の `VaultMode::Game` から呼ばれる。

```toml
# dc34-vault/Cargo.toml
badge-game = { package = "dc34-virtual-pet", path = "../dc34-virtual-pet" }
```

`package =` のエイリアスを使っているので、別のゲームに差し替えるときは
この 1 行を書き換えるだけでよく、`dc34-vault` 側のコードは変更不要。

ホストが知っているのは [`BadgeGame`](src/lib.rs) trait と `new_game()` だけで、
具体的なゲーム型を名指ししない。ゲームは画面・キー・時計を受け取り、
`GameAction` を返す。**vault のモードを直接変更する手段は持たない**ので、
ゲーム側の不具合でバッジが操作不能になることはない。

```rust
pub trait BadgeGame {
    fn start(&mut self, now_ms: u64);
    fn tick(&mut self, now_ms: u64);
    fn key(&mut self, k: char) -> GameAction;
    fn draw(&self, gfx: &Gfx);
}
```

### 呼び出し側に必要なこと

- `tick()` を定期的に呼ぶこと。**ゲーム内の時間はこれでしか進まない**
- `draw()` の直後に flush すること（このクレートは flush しない）
- `key()` が `GameAction::Exit` を返したら、ゲームモードから抜けること

## ビルドの前提

`Cargo.toml` が `../xous-core` のような相対パスで参照し合っているため、
以下が同じ階層に並んでいる必要がある。

```
<同一ディレクトリ>/
├── xous-core          (betrusted-io/xous-core)
├── dc34-api           (bunnie/dc34-api)
├── dc34-vault         (bunnie/dc34-vault またはその fork)
└── dc34-virtual-pet   (このリポジトリ)
```

**cargo はシンボリックリンクを canonicalize して実パスで解決する**ため、
`~/dev/dc34` のような作業用ディレクトリを作ってそこにリンクを並べても解決しない。
リンクは各リポジトリの実パスの隣に置く必要がある。

## 依存の方針

配布を見据えて、**恒久的に改変するのは `dc34-vault` とこのリポジトリだけ**に留める。
`xous-core` / `dc34-api` / `dc34-console` は公式のまま使う。
