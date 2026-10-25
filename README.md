# Kwel for Alfred

Search your Chrome browsing history from Alfred using [Kwel](https://kwel.ryu.zone).

Requires macOS, Alfred 5 Powerpack, Google Chrome, and Kwel with Alfred Integration.

## Install

1. Download **Kwel.alfredworkflow** from [GitHub Releases](https://github.com/Masa-Ryu/kwel-alfred/releases).
2. Double-click the file to import it into Alfred.
3. Enable **Alfred Integration** in Kwel Settings and copy the **Profile ID**.
4. Open the Workflow's **Configure Workflow…** and paste the ID into **Kwel Profile ID**.
5. In Alfred Preferences → **Features → Default Results → Setup fallback searches**, add **Search Kwel** and save.

The first **Search Kwel** invocation registers the connection. Keep Chrome open; results appear automatically when Kwel connects. If it remains disconnected, check Alfred Integration and copy its current Profile ID into the Workflow configuration.

## Use

Open Alfred, type `Kyoto`, choose **Search Kwel**, and press Return. Select a page and press Return to open it in Chrome.
