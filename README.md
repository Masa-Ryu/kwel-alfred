# Kwel Connect

Kwelの検索をローカルアプリから使うための公式連携基盤です。macOS・Windows・Ubuntu向けのNative Host、JSON CLI、公式 **Kwel for Alfred** を提供します。

必要なもの：Google Chromeと、Local Connectionに対応した[Kwel](https://kwel.ryu.zone)。Alfredを使う場合はmacOSとAlfred 5 Powerpackも必要です。

## インストール

[GitHub Releases](https://github.com/Masa-Ryu/kwel-connect/releases)から、お使いのOSに合うファイルをダウンロードします。

| OS | インストーラー |
| --- | --- |
| macOS 13以降（Apple Silicon / Intel） | `KwelConnect-macos-universal.pkg` |
| Windows x64 | `KwelConnect-windows-x64-setup.exe` |
| Ubuntu 24.04以降 amd64 | `KwelConnect-ubuntu-amd64.deb` |
| Ubuntu 24.04以降 arm64 | `KwelConnect-ubuntu-arm64.deb` |

macOSではpkgを開いて自分のホームフォルダへ、Windowsではexeを開いて現在のユーザーへインストールします。Ubuntuでは対応するdebをパッケージインストーラーで開きます。Host登録は自動で行われ、Rustのインストールは不要です。

macOSの利用条件は[現行Chromeの対応OS](https://support.google.com/chrome/answer/95411)に合わせています。

更新時は、使用中のChromeプロフィールでLocal ConnectionをOFFにしてからインストーラーを開き、更新後にONへ戻します。

Kwel Settingsで **Local Connection** をONにします。Chromeの権限確認が表示された場合は内容を確認して承認してください。

配布物は未署名・未公証です。GatekeeperやSmartScreenでブロックされる場合があります。OSの確認手順で配布元を確認してください。

### macOSで初回起動時に警告が表示される場合

Kwel ConnectのmacOS配布物は未署名・未公証です。初回実行時にmacOSが個別の実行ファイルをブロックする場合があります。

対象となるファイル：

- `kwel-native-host`：ChromeとのNative Messaging接続時に実行
- `kwel-bridge`：ローカルCLIの実行時に使用
- `kwel-alfred`：Alfred Workflowによる検索時に実行

各ファイルは異なるタイミングで起動するため、初回確認が複数回必要になる場合があります。

公式[GitHub Releases](https://github.com/Masa-Ryu/kwel-connect/releases)から取得したファイルであることと、公開されたSHA-256チェックサムとの一致を確認してください。

未確認の開発元・未公証による警告の場合、対象ファイルの起動を試みた後、Appleが案内する「システム設定 → プライバシーとセキュリティ → このまま開く（Open Anyway）」で、対象ファイルごとに開くかどうかを判断できます。

「マルウェアが検出された」「コンピュータに損害を与える」など、別のセキュリティ警告の場合は許可を進めず、正確な警告文を確認してください。

Apple公式案内：[Macでアプリを安全に開く](https://support.apple.com/ja-jp/102445)

## Kwel for Alfred

1. Releasesの **Kwel.alfredworkflow** をダブルクリックしてインポートします。
2. Kwel SettingsのProfile IDをコピーし、Workflowの **Configure Workflow… → Kwel Profile ID** に設定します。
3. Alfred Preferences → **Features → Default Results → Setup fallback searches** で **Search Kwel** を追加します。

Alfredに`Kyoto`などの検索語を入力し、**Search Kwel** を選びます。結果を選んでReturnを押すと、そのChromeプロフィールでページが開きます。接頭辞や追加ホットキーは必要ありません。

Alfredのみを使う場合、初回検索でHostが自動登録されるため、pkgの事前インストールは不要です。Workflowを削除しても共通Hostは残ります。

## 削除

Local ConnectionをOFFにしてから、macOSでは`~/Library/Application Support/Kwel/NativeBridge/`内の **Kwel Connect Uninstaller**、Windowsでは設定のアプリ一覧、Ubuntuではパッケージマネージャーから削除します。使用中のHostは削除を拒否し、Kwelの履歴やプロフィールは保持します。

## ランチャー開発者向け

[Local Client Protocol v1](docs/local-client-protocol-v1.md)は他言語から利用できます。[Native Messaging仕様](docs/native-messaging-v1.md)、[セキュリティ](docs/security.md)、[開発手順](docs/development.md)も参照してください。

公式のサポート対象は3 OS向けHostとAlfred Workflowです。第三者ランチャーの実装・品質・動作は、その開発者の責任となります。
