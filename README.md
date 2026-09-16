# Terracotta

Terraformのplanを楽に読んで判断しやすくするためのCLI。

Gitの変更と突き合わせて、**「自分のコード変更だけでは説明しづらい差分」**を見つけやすくする。

> まだ開発中。Terraformの実行機能は未実装。

## イメージ

- **一時CLI**：使うときだけ開き、終わったらシェルに戻る
- **Git変更との照合**：各リソースが今のコード変更とつながるかを見る
- **確認したplanのまま進む**：見たsaved planを、再planせずにapplyする

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
  apply / quit

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

直接変えていないリソースでも、辿れるなら経路だけ出す。

```text
locals.common_tags
  ↓
module.ecs
  ↓
aws_ecs_service.app
```

変更原因の断定ではない。Terraform設定上の対応関係。

### 確認したplanのまま進む

レビューしたsaved planを、そのままapplyする。
失敗したら成功済み、失敗、diagnostic、plan差分、Git対応を同じ場で見る。

普通の `terraform plan` に戻りたくなくなるかどうか。
