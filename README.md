# AssetCrunch v0.2.0

GPU-accelerated game asset pack optimizer for indie developers.

ゲームアセットフォルダを解析し、ファイル種別ごとに最適な圧縮を適用。
配布前の素材フォルダを自動で最適化するCLIツールです。


![画像］()
![Pack実行結果](https://github.com/user-attachments/assets/b2e123b5-b281-4b7c-941f-9b804e31510b)

## 実測圧縮率

| フォーマット | 削減率 | 備考 |
|---|---|---|
| JSON / TXT | **75.6%** | シーンデータ・設定ファイル |
| OBJ / FBX | **40.6%** | 3Dメッシュ |
| WAV | **17.9%** | ボイス・SE・BGM |
| PNG / MP3 / OGG | スキップ | 圧縮済みフォーマット |
| 圧縮逆効果ファイル | 自動スキップ | 元ファイルをコピー |

## 動作環境

- Windows / Linux / macOS
- Vulkan / DX12 / Metal 対応GPU
- Rust 1.75以上

## インストール

```bash
git clone https://github.com/koara53/AssetCrunch
cd AssetCrunch
cargo build --release
```

または Releases からビルド済みexeをダウンロード。

## Windowsの右クリックメニューに登録

管理者権限のPowerShellで：

```powershell
.\install.ps1
```

フォルダを右クリック → AssetCrunch → Pack（最適化）/ 解凍 で使えます。

## 使い方

### メイン機能

```bash
# フォルダ全体を最適化パック（推奨）
assetcrunch pack   <input_dir> <output_dir>

# パックを元のフォルダ構造に完全復元
assetcrunch unpack <input_dir> <output_dir>
```

`pack` コマンドは以下を自動で行います：
- ファイル種別を解析
- 圧縮対象は GPU LZ4 で圧縮
- 圧縮済みフォーマット（PNG/MP3等）はそのままコピー
- 圧縮が逆効果なファイルも自動でコピー
- `assetcrunch_manifest.json` を生成（unpackで完全復元可能）
- 完了後にレポートをポップアップ表示

### 単体ファイル操作

```bash
assetcrunch compress         <input.wav>      <output.gcwav>
assetcrunch decompress       <input.gcwav>    <output.wav>
assetcrunch compress-mesh    <input.obj/fbx>  <output.gcmesh>
assetcrunch decompress-mesh  <input.gcmesh>   <output_dir>
assetcrunch compress-text    <input.json/txt> <output.gcmesh>
assetcrunch decompress-text  <input.gcmesh>   <output_dir>
```

### その他

```bash
assetcrunch --bench  # ベンチマーク実行
```

## 対応フォーマット

| 種別 | 拡張子 | 処理 |
|---|---|---|
| 音声 | wav | デルタ符号化 + バイトプレーン分離 + GPU LZ4 |
| 3Dメッシュ | obj, fbx | GPU LZ4 |
| テキスト系 | json, txt, csv, xml | GPU LZ4 |
| 圧縮済み | png, jpg, mp3, ogg, webp | スキップ（コピー） |
| 逆効果 | 上記以外で膨らむもの | 自動スキップ（コピー） |

## 技術仕様

- GPU: WGSLコンピュートシェーダー（wgpu経由でVulkan/DX12/Metal対応）
- 圧縮: LZ4 block format（GPU並列実装・サブチャンク4分割）
- WAV前処理: バイトプレーン分離 + デルタ符号化
- チャンクサイズ: 64KB（LZ4最大ウィンドウサイズ）
- GPU speedup: テクスチャ系データでCPU比 **3.84倍**

## ライセンス

MIT
