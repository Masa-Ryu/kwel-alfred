#!/usr/bin/env python3
"""Exercise real installers only in disposable hosted CI user accounts."""
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tomllib

ROOT=Path(__file__).resolve().parents[1]
VERSION=tomllib.loads((ROOT/'Cargo.toml').read_text())['workspace']['package']['version']
HOSTS=('com.kwel.bridge','com.kwel.alfred')
def run(*args,**kwargs):return subprocess.run([str(value) for value in args],check=True,timeout=180,**kwargs)
def manifests(directory,binary):
    for name in HOSTS:
        value=json.loads((directory/f'{name}.json').read_text())
        assert value['name']==name and Path(value['path'])==binary and value['type']=='stdio'
        assert value['allowed_origins']==['chrome-extension://mhablbmgafbdofgaffhdnnknjhafmlin/']
def main():
    if os.environ.get('GITHUB_ACTIONS')!='true' or os.environ.get('RUNNER_ENVIRONMENT')!='github-hosted':
        raise SystemExit('Installer tests require a disposable GitHub-hosted runner.')
    if sys.platform=='darwin':
        package=ROOT/'dist/KwelConnect-macos-universal.pkg'
        domains=run('installer','-dominfo','-pkg',package,capture_output=True,text=True).stdout
        assert 'CurrentUserHomeDirectory' in domains
        root=Path.home()/'Library/Application Support/Kwel/NativeBridge'
        chrome=Path.home()/'Library/Application Support/Google/Chrome/NativeMessagingHosts'
        for _ in range(2):run('installer','-pkg',package,'-target','CurrentUserHomeDirectory')
        manifests(chrome,root/'bin/kwel-native-host')
        assert run(root/'bin/kwel-native-host','--version',capture_output=True,text=True).stdout.strip()==VERSION
        assert run(root/'bin/kwel-bridge','--version',capture_output=True,text=True).stdout.strip()==VERSION
        run(root/'bin/kwel-native-host','uninstall')
        assert not (root/'bin/kwel-native-host').exists()
        for name in HOSTS:assert not (chrome/f'{name}.json').exists()
    elif sys.platform=='win32':
        import winreg
        root=Path(os.environ['LOCALAPPDATA'])/'Kwel/NativeBridge'
        for _ in range(2):run(ROOT/'dist/KwelConnect-windows-x64-setup.exe','/S')
        manifests(root/'manifests',root/'bin/kwel-native-host.exe')
        for name in HOSTS:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER,rf'Software\Google\Chrome\NativeMessagingHosts\{name}') as key:
                assert Path(winreg.QueryValueEx(key,'')[0])==root/'manifests'/f'{name}.json'
        assert run(root/'bin/kwel-native-host.exe','--version',capture_output=True,text=True).stdout.strip()==VERSION
        run(root/'Uninstall.exe','/S',f'_?={root}')
        assert not (root/'bin/kwel-native-host.exe').exists()
        for name in HOSTS:
            try:
                with winreg.OpenKey(winreg.HKEY_CURRENT_USER,rf'Software\Google\Chrome\NativeMessagingHosts\{name}'):
                    raise AssertionError('Native Host registry entry remains')
            except FileNotFoundError:pass
    else:
        architecture={'x86_64':'amd64','aarch64':'arm64'}[platform.machine()]
        package=ROOT/f'dist/KwelConnect-ubuntu-{architecture}.deb'
        for _ in range(2):run('sudo','dpkg','--install',package)
        manifests(Path('/etc/opt/chrome/native-messaging-hosts'),Path('/usr/lib/kwel-connect/kwel-native-host'))
        assert run('/usr/bin/kwel-bridge','--version',capture_output=True,text=True).stdout.strip()==VERSION
        run('sudo','dpkg','--remove','kwel-connect')
        assert not Path('/usr/lib/kwel-connect/kwel-native-host').exists()
        for name in HOSTS:assert not Path(f'/etc/opt/chrome/native-messaging-hosts/{name}.json').exists()
    print('Installer registration, idempotent upgrade and owned removal verified.')
if __name__=='__main__':main()
