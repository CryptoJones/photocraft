# PhotoCraft in Mozilla Pontoon

PhotoCraft's Fluent catalogs are ready for a Pontoon project. The [repository
configuration](../l10n.toml) points to the English source and six locale
directories. `zh-CN` is reviewed for Mainland China usage; `zh-TW` is reviewed
for Taiwan usage. The app continues to use `zh-hans` and `zh-hant` internally.

## Connect an instance

An administrator of the selected Pontoon instance must create the project and
give Pontoon Git write access. Mozilla's public instance has its own project
approval process; otherwise use a separately hosted Pontoon instance. The
[Pontoon deployment guide](https://pontoon.mozilla.org/docs/dev/deployment/)
describes production hosting, and its Docker quickstart is for development.

In Pontoon's **Add New Project** form, use:

| Field | Value |
| --- | --- |
| Name | PhotoCraft |
| Repository | `git@github.com:storytold/photocraft.git` |
| Configuration file | `l10n.toml` |
| Locales | Read list of locales from repository |
| Public repository website | `https://github.com/storytold/photocraft` |
| Visibility | Private until the first sync is verified |

Pontoon should connect to the repository's default branch after this PR is
merged. In the project administrator UI, run **SYNC**, inspect the sync log,
verify that `messages.ftl` appears for each locale, and check that the Chinese
catalogs import all 2,434 message IDs. Then make the project public for
proofreading. Follow Pontoon's recommendation to use a dedicated GitHub account
and SSH key for its repository access; keep credentials outside this repository.

## Review and merge

The [screenshot gallery](images/i18n/README.md) shows the menus in both Chinese
locales. Preserve Fluent variables and selectors, and use the regional glossary
in [Chinese localization guidance](localization-zh-hans.md). Run
`cargo xtask i18n check`, UI tests, and screenshot review for each imported
translation update. The `i18n check` command also checks that `l10n.toml`
still points to the catalogs embedded by the app.

Pontoon's [project setup guide](https://pontoon.mozilla.org/docs/admin/adding-new-project/)
describes repository permissions, locale discovery, and the initial sync.
