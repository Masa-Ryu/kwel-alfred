# Kwel for Alfred

Search your Chrome browsing history from Alfred using [Kwel](https://kwel.ryu.zone).

Requires macOS, Alfred 5 Powerpack, Google Chrome, and Kwel with Alfred Integration.

## Setup

1. Enable **Alfred Integration** in Kwel Settings and copy the **Profile ID**.
2. Import `Kwel.alfredworkflow`. Open **Configure Workflow…** and paste the ID into **Kwel Profile ID**.
3. Add **Search Kwel** under Alfred Preferences → **Features → Default Results → Setup fallback searches**.

The first **Search Kwel** invocation registers the connection. If Kwel stays disconnected, turn **Alfred Integration** off and on, then retry.

## Use

Open Alfred, type `Kyoto`, choose **Search Kwel**, and press Return. Select a page and press Return to open it in Chrome.

To use another Chrome profile, change **Kwel Profile ID** in Workflow Configuration.

## Build

With Go 1.22+, Python 3, and macOS command line tools:

```sh
bash scripts/package-workflow.sh
bash scripts/verify.sh
```

The package is written to `dist/Kwel.alfredworkflow`.

[MIT License](LICENSE).
