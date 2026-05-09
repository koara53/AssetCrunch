\#アセットクランチ



インディー開発者向け、GPUアクセラレーション対応ゲームアセット圧縮ツール。



ふりーむ・itch.io向けのゲームアセットをGPUボーダーLZ4で高速圧縮するCLIツールです。



\## 実測圧縮率



| フォーマット | 削減率 | メモ |



|---|---|---|



| JSON / TXT | \*\*75.6%\*\* | シーンデータ・設定ファイル |



| OBJ/FBX | \*\*40.6%\*\* | 3Dメッシュ |



| WAV | \*\*17.9%\*\* | ボイス・SE・BGM |



| PNG / MP3 / OGG | スキップ | 圧縮済みフォーマット |



\##動作環境



\- Windows / Linux / macOS



\- Vulkan / DX12 / Metal対応GPU



\- 錆び1.75以上



\## インストール



```bash



git clone https://github.com/YOUR\\\_USERNAME/AssetCrunch



cd AssetCrunch



cargo build --release



\\```



\##使い方



```bash



\# WAV圧縮・解凍



assetcrunch compress <input.wav> <output.gcwav>



assetcrunch decompress <input.gcwav> <output.wav>



\# 3Dメッシュ圧縮・解凍(OBJ / FBX)



assetcrunch compress-mesh <input.obj/fbx> <output.gcmesh>



assetcrunch decompress-mesh <input.gcmesh> <output\_dir>



\#テキスト系圧縮・解凍(JSON / TXT)



assetcrunch compress-text <input.json/txt> <output.gcmesh>



assetcrunch decompress-text <input.gcmesh> <output\_dir>



\#フォルダ一括処理



assetcrunch compress-folder <input\_dir> <output\_dir>



\#ベンチマーク



assetcrunch --bench



\\```



\## 対応フォーマット



| 種類 | 拡張子 | 処理 |



|---|---|---|



| 音声 |ウェーブ | デルタ記号化 + バイトプレーン分離 + GPU LZ4 |



| 3Dメッシュ |オブジェクト、FBX | GPU LZ4 |



| テキスト系 | json、txt | GPU LZ4 |



| 圧縮済み | png、jpg、mp3、ogg | スキップ（膨張防止） |



\## 技術仕様



\- GPU：WGSLコンピュートシェーダー（wgpu経由でVulkan/DX12/Metal対応）



\- 圧縮：LZ4ブロック形式（GPU実装）



\- WAV前処理:バイトプレーン分離+デルタ記号化で圧縮率を改善



\- チャンクサイズ：64KB（LZ4最大ウィンドウサイズ）



\## ライセンス



MIT

