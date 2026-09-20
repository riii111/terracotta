# Terracotta

Terraformのplanを全文で確認し、確認したsaved planをそのままapplyできる、一時的なTUI付きCLI。

## 使い方

Terraformを初期化済みのrootで、対話的な端末から実行する。

```sh
terracotta plan
```

Terracottaはその場で`terraform plan -out=<一時ファイル>`を実行し、終了後にplan全文を確認できる画面を開く。planの内容と対象のworkspaceを確認してから、必要ならapplyへ進む。

確認画面では次の操作を使う。

- `↑`/`↓`/`←`/`→`: planをスクロール
- `/`: plan全文を検索
- `y`: 機微値をマスクしたplanをclipboardへコピー
- `a`: apply確認へ進む
- `q`: applyせずに終了

apply確認では、入力欄に小文字の`yes`または`no`を入力してEnterを押す。`yes`で確認した一時planを1回だけapplyし、`no`またはEscでplan確認へ戻る。大文字や別の入力ではapplyを開始しない。

apply中はTerraformの標準出力と標準エラーを表示する。完了後は成功・失敗・中断の状態、Terraformの出力、結果のclipboardコピーを確認できる。applyを開始した後は再applyや再planを行わない。

applyを実行せずに終了した場合の終了コードは0。applyが失敗した場合は1、Ctrl-Cで中断した場合は130。失敗または中断では、変更の一部が適用済みの可能性がある。

## 画面の流れ

```text
$ terracotta plan

      ↓

  terraform init済みのroot
      ↓
  plan実行
      ↓
  plan全文レビュー
      ↓
  apply確認（任意）
      ↓
  apply結果とTerraform出力
      ↓

$ shell
```

Terracottaは常駐せず、確認が終わるとシェルへ戻る。クラウド接続、state、認証情報など、通常のTerraform実行に必要な環境は利用者が用意する。
