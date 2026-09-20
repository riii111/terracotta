# Terracotta

Terraformのplanを読み、判断しやすくする、確認したplanをapplyできる一時的なTUI付きCLI。

Gitの変更と突き合わせて、**「自分のコード変更だけでは説明しづらい差分」**を見つけやすくする。

MVPは`terracotta plan`によるplan実行、変更一覧、resource詳細、Gitのdirect照合、機微値の一時表示、マスク済みコピー、確認したplanのapplyを提供する。確認を終えるとシェルへ戻る。

## 使い方

Terraformを初期化済みのrootで実行する。

```sh
terracotta plan
```

通常は作業ツリーと`HEAD`を比較する。基準ブランチとの差分を確認する場合は、`HEAD`と指定refのmerge-baseを比較する。

```sh
terracotta plan --compare-ref main
```

Terraformのplan失敗時はdiagnosticを表示して終了を待つ。Gitの取得・解析だけが失敗した場合は、planを閲覧できる状態を保ったまま「解析不完全」と表示する。

planに変更がある場合、確認画面で`a`を押すとapply確認へ進む。`yes`とEnterで、確認した一時planを1回だけapplyする。`no`とEnter、またはEscでplan確認へ戻る。apply中はTerraformの標準出力と標準エラーを表示し、完了後は成功・失敗・中断の結果を確認できる。

applyを実行せずに`q`で終了した場合の終了コードは0。applyが失敗した場合は1、Ctrl-Cで中断した場合は130。失敗または中断では、変更の一部が適用済みの可能性がある。

`plan`はクラウド接続を隠す機能ではない。対象rootのTerraform設定、state、認証情報など、通常の`terraform plan`と`terraform apply`に必要な環境を用意する。

## イメージ

- **一時CLI**：使うときだけ開き、終わったらシェルに戻る
- **Git変更との照合**：各リソースが今のコード変更とつながるかを見る
- **確認したplanを持ち出す**：機微値をマスクしたplanやresourceをclipboardへコピーする
- **確認したplanをapplyする**：保存済みplanを1回だけTerraformへ渡す

### 一時CLI

`terracotta plan` のあいだだけTUIを開く。
常駐しない。Homeやworkspace管理画面も持たない。

```text
$ terracotta plan

      ↓

  progress
      ↓
  plan review
      ↓
  apply (optional)
      ↓
  result

      ↓

$ shell
```

`init` が必要でも管理画面にはしない。その場で実行するかやめるか。

### Git変更との照合

各リソースを、今のGit変更と並べて見る。

```text
Plan

+1  ~1  ↻0  -1

Needs review: 1
compare: working tree vs HEAD

+ aws_subnet.private
  Git: network.tf:18

~ aws_instance.api
  Git: main.tf:42

- aws_security_group.old
  Git: no match             ⚠
```

`Git: no match` は「無関係」ではない。
**今のコード変更との対応が見つからなかった**、という表示。
原因や安全性は判断しない。見る場所を絞る。

リソースを選ぶと、コード変更とplan差分をまとめて見られる。

```text
~ aws_instance.api

Git:
  main.tf:42

Diff:
  instance_type
    t3.small
    → t3.medium
```

MVPではroot直下のmanaged resourceとネイティブHCLのGit変更をdirectに照合する。間接的な影響経路の推定は行わない。

### 確認したplanを持ち出す

plan全体やresource単位の情報を、機微値をマスクしたテキストとしてclipboardへコピーできる。apply結果もTerraformの出力を保ったままclipboardへコピーできる。
