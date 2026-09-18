# Terracotta

Terraformのplanを楽に読んで判断しやすくする、一時的なTUI付きCLI。

Gitの変更と突き合わせて、**「自分のコード変更だけでは説明しづらい差分」**を見つけやすくする。

MVPは`terracotta plan`によるplan実行、変更一覧、resource詳細、Gitのdirect照合、機微値の一時表示、マスク済みコピーを提供する。確認を終えるとシェルへ戻る。

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

`plan`はクラウド接続を隠す機能ではない。対象rootのTerraform設定、state、認証情報など、通常の`terraform plan`に必要な環境を用意する。

## イメージ

- **一時CLI**：使うときだけ開き、終わったらシェルに戻る
- **Git変更との照合**：各リソースが今のコード変更とつながるかを見る
- **確認したplanを持ち出す**：機微値をマスクしたplanやresourceをclipboardへコピーする

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
  quit

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

plan全体やresource単位の情報を、機微値をマスクしたテキストとしてclipboardへコピーできる。

普通の`terraform plan`に戻りたくなくなるかどうか。
