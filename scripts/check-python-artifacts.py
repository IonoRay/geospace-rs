#!/usr/bin/env python3
"""Inspect actual Python artifacts and run the wheel contracts in an isolated site.
No installation or network access. macOS dependency checks use otool.
"""
import argparse
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile

ROOT=Path(__file__).resolve().parents[1]

def check_wheel(wheel, models, full_suite):
    with tempfile.TemporaryDirectory(prefix='ionoray-wheel-check-',dir='/tmp') as directory:
        site=Path(directory).resolve()
        with zipfile.ZipFile(wheel) as archive:
            archive.extractall(site)
            names=archive.namelist()
            metadata_name=next(n for n in names if n.endswith('/METADATA'))
            metadata=archive.read(metadata_name).decode()
            expected=['LICENSE','LICENSE-MIT','LICENSE-APACHE','THIRD_PARTY_NOTICES.md','crates/models/iri/IRI-LICENSE.txt','crates/models/msis/nrlmsis2.1_license.txt','crates/indices/GFZ-NOTICE.txt']
            expected += [p.relative_to(ROOT).as_posix() for p in sorted((ROOT/'licenses/native').glob('*.txt'))]
            for license in expected:
                name=metadata_name.rsplit('/',1)[0]+'/licenses/'+license
                assert name in names, f'missing wheel license: {license}'
                assert archive.read(name)==(ROOT/license).read_bytes(),f'outdated wheel license: {license}'
            assert 'License-Expression: LicenseRef-IonoRay-Distribution' in metadata
        libraries=[]
        for file in site.rglob('*'):
            if file.suffix in ['.so','.dylib'] and sys.platform=='darwin':
                dependencies=subprocess.check_output(['/usr/bin/otool','-L',str(file)],text=True)
                assert '/nix/store/' not in dependencies,f'unrepaired wheel: {dependencies}'
                libraries.append(file.name)
        expected=set(models.split(',')) if models else set()
        code=f"import sys;sys.path[:0]=[{str(site)!r},{str(ROOT)!r}];import ionoray_geospace as gs;assert set(gs.capabilities())=={expected | {'indices'}!r};import unittest;suite=unittest.defaultTestLoader.loadTestsFromNames({(['python.tests.test_capabilities','python.tests.test_models','python.tests.test_session','python.tests.test_scenarios','python.tests.test_analysis'] if full_suite else ['python.tests.test_capabilities'])!r});result=unittest.TextTestRunner(verbosity=2).run(suite);sys.exit(not result.wasSuccessful())"
        env={k:v for k,v in os.environ.items() if not k.startswith(('NIX_','IONORAY_','DYLD_','PYTHONPATH','PYTHONHOME'))}
        # Child-interpreter lifecycle/example tests must import this same wheel.
        env['PYTHONPATH']=str(site)
        subprocess.run([sys.executable,'-I','-c',code],cwd=site,env=env,check=True,timeout=180)
        print('wheel:',wheel.name,'models:',sorted(expected),'dynamic libraries:',libraries)

def check_sdist(path):
    spec=importlib.util.spec_from_file_location('sync_cache',ROOT/'scripts/sync-cache.py')
    cache=importlib.util.module_from_spec(spec);spec.loader.exec_module(cache)
    with tarfile.open(path) as archive:
        names=archive.getnames();prefix=names[0].split('/')[0]+'/'
        for forbidden in cache.ASSET_GROUPS:
            assert not any(n.endswith('/'+forbidden) for n in names),f'sdist source snapshot: {forbidden}'
        for required in ['Cargo.toml','crates/python/build.rs','assets/cache-manifest.json','scripts/sync-cache.py','docs/releases.md','docs/main-cache-validation.md','flake.nix','flake.lock','LICENSE-MIT','LICENSE-APACHE','THIRD_PARTY_NOTICES.md']:
            assert prefix+required in names,f'sdist omits {required}'
        workspace=tomllib.loads((ROOT/'Cargo.toml').read_text())
        members=set(workspace['workspace']['members'])
        members.update(dep['path'] for dep in workspace['workspace']['dependencies'].values()
                       if isinstance(dep,dict) and 'path' in dep)
        for member in sorted(members):
            assert prefix+member+'/Cargo.toml' in names,f'sdist omits workspace member: {member}'
            for path in ['LICENSE-MIT','LICENSE-APACHE','THIRD_PARTY_NOTICES.md']:
                assert prefix+member+'/'+path in names,f'sdist omits member notice: {member}/{path}'
    print('sdist:',len(names),'entries, no snapshots, common source and notices included')

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--wheel',type=Path)
    parser.add_argument('--models',default='')
    parser.add_argument('--full-suite',action='store_true')
    parser.add_argument('--sdist',type=Path)
    args=parser.parse_args()
    if args.wheel:check_wheel(args.wheel,args.models,args.full_suite)
    if args.sdist:check_sdist(args.sdist)
    if not args.wheel and not args.sdist:parser.error('select --wheel and/or --sdist')
