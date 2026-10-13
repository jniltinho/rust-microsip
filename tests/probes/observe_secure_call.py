"""Separate-process observation of the finite TLS/SDES RTP/SRTCP/rekey experiment."""
if not __debug__:
    raise SystemExit('Optimized Python is not permitted for evidence verification')

import hashlib,json,math,os,pathlib,re,subprocess,sys,time
root=pathlib.Path(__file__).resolve().parents[2];os.chdir(root)
name=sys.argv[1] if len(sys.argv)==2 else 'secure-call-work'
if len(sys.argv)>2 or not re.fullmatch(r'[a-z0-9-]{1,64}',name):
 raise SystemExit('Expected one neutral artifact directory name')
if (root/'artifacts').is_symlink(): raise SystemExit('Symlinked artifact root')
work=root/'artifacts'/name
prefix=root/'artifacts'/f'{name}-runtime'
env=dict(os.environ,LAB_UID=str(os.getuid()),LAB_GID=str(os.getgid()))
base=['docker','compose','-f','docker/asterisk/compose.yaml','-f','docker/asterisk/compose.calls-g0.yaml']
def run(args): return subprocess.check_output(args,env=env,text=True,stderr=subprocess.STDOUT,timeout=30)
def save(suffix,obj): pathlib.Path(str(prefix)+suffix).write_text(json.dumps(obj,indent=2)+'\n')
def counts(text):
 a=re.findall(r'^\s*(\d+) active channels?\s*$',text,re.M);b=re.findall(r'^\s*(\d+)(?: of \d+ max)? active calls?(?: \(\s*\d{1,3}\.\d{2}% of capacity\))?\s*$',text,re.M)
 if len(a)!=1 or len(b)!=1: raise RuntimeError('ambiguous channel counts')
 return int(a[0]),int(b[0])
assert counts('2 active channels\n1 active call\n')==(2,1)
assert counts('0 active channels\n0 active calls\n')==(0,0)
assert counts('0 active channels\n1 active call\n')!=(0,0)
for bad in ['none','0 active channels\n0 active channels\n0 active calls\n','0 active channels\n0 active calls suffix']:
 try: counts(bad)
 except RuntimeError: pass
 else: raise AssertionError('malformed counts accepted')
prepared=json.loads((work/'prepared-source-inputs.json').read_text())
assert prepared==json.loads((root/'tests/probes/secure_call_inputs.json').read_text())['candidate_inputs_sha256'] and len(prepared)==39
for relative,digest in prepared.items():
 path=work/relative
 assert path.is_file() and not any(p.is_symlink() for p in (path,*path.parents))
 assert hashlib.sha256(path.read_bytes()).hexdigest()==digest
elf=work/'target/x86_64-unknown-linux-gnu/release/examples/secure_call'
assert elf.is_file() and not any(p.is_symlink() for p in (elf,*elf.parents))
elf_hash=hashlib.sha256(elf.read_bytes()).hexdigest();image=run(['docker','image','inspect','rust-microsip-toolchain:1.99.0','--format','{{.Id}}']).strip()
pbx_image=run(['docker','image','inspect','rust-microsip-asterisk:22.11.0-lab.1','--format','{{.Id}}']).strip()
assert not run(['docker','ps','-aq','--filter','label=com.docker.compose.project=rust-microsip-lab']).strip(), 'Disposable project is already in use'
assert not run(['docker','network','ls','-q','--filter','label=com.docker.compose.project=rust-microsip-lab']).strip(), 'Disposable project network is already in use'
process=None;proof={}
try:
 run(base+['up','--no-build','-d','--wait','pbx'])
 net=json.loads(run(['docker','network','inspect','rust-microsip-lab_isolated']))[0];assert net['Internal']
 pbx=run(base+['ps','-q','pbx']).strip();assert pbx
 pbxi=json.loads(run(['docker','inspect',pbx]))[0]
 assert pbxi['Image']==pbx_image and not pbxi['HostConfig']['PortBindings']
 assert pbxi['Config']['User']==f'{os.getuid()}:{os.getgid()}' and pbxi['HostConfig']['ReadonlyRootfs']
 assert pbxi['HostConfig']['CapDrop']==['ALL'] and 'no-new-privileges:true' in pbxi['HostConfig']['SecurityOpt']
 assert [(m['Destination'],m['RW']) for m in pbxi['Mounts'] if m['Type']=='bind']==[('/lab',False)]
 def pbxcli(s): return run(['docker','exec',pbx,'asterisk','-C','/run/asterisk/config/asterisk.conf','-rx',s])
 for account in ('1201','1202'):
  endpoint=pbxcli('pjsip show endpoint '+account)
  assert re.search(r'media_encryption\s*:\s*sdes',endpoint)
  assert re.search(r'media_encryption_optimistic\s*:\s*false',endpoint)
  assert re.search(r'rewrite_contact\s*:\s*false',endpoint)
 assert re.search(r'res_srtp\.so\s+.*Running',pbxcli('module show like res_srtp.so'))
 args=['docker','run','--rm','--name','rust-microsip-secure-call-probe','--label','com.docker.compose.project=rust-microsip-lab','--network','rust-microsip-lab_isolated','--user',f'{os.getuid()}:{os.getgid()}','--read-only','--cap-drop','ALL','--security-opt','no-new-privileges','--memory','512m','--cpus','2','--pids-limit','160','--tmpfs','/tmp:rw,noexec,nosuid,nodev,mode=1777,size=32m','--mount',f'type=bind,source={elf},target=/probe,readonly','--mount',f'type=bind,source={root}/artifacts/asterisk-lab/clients,target=/lab,readonly',image,'/probe']
 observed=False;hardened=False
 with pathlib.Path(str(prefix)+'.log').open('w') as log:
  process=subprocess.Popen(args,env=env,stdout=log,stderr=subprocess.STDOUT)
  deadline=time.monotonic()+160
  while process.poll() is None and time.monotonic()<deadline:
   if not hardened:
    try:
     obj=json.loads(run(['docker','inspect','rust-microsip-secure-call-probe']))[0]
     assert obj['Image']==image
     if obj['State']['Status']!='running': time.sleep(0.1);continue
     assert obj['HostConfig']['ReadonlyRootfs'] and [x.upper() for x in obj['HostConfig']['CapDrop']]==['ALL'] and any(x in ('no-new-privileges','no-new-privileges:true') for x in obj['HostConfig']['SecurityOpt']) and not obj['HostConfig']['PortBindings']
     assert obj['Config']['User']==f'{os.getuid()}:{os.getgid()}'
     assert sorted((m['Destination'],m['RW']) for m in obj['Mounts'] if m['Type']=='bind')==[('/lab',False),('/probe',False)]
     assert run(['docker','exec','rust-microsip-secure-call-probe','sha256sum','/probe']).split()[0]==elf_hash
     status=run(['docker','exec','rust-microsip-secure-call-probe','cat','/proc/1/status'])
     assert re.search(r'^NoNewPrivs:\s+1$',status,re.M) and re.search(r'^CapEff:\s+0+$',status,re.M)
     hardened=True
    except subprocess.CalledProcessError: pass
   if counts(pbxcli('core show channels count'))==(2,1): observed=True
   time.sleep(0.5)
  code=process.wait(timeout=10)
 if code: raise RuntimeError('secure runner failed; see redacted ignored log')
 lines=[json.loads(s) for s in pathlib.Path(str(prefix)+'.log').read_text().splitlines()]
 matrix=lines[-1];rows=matrix['calls']
 assert len(rows)==4 and {(r['codec'],r['reverse']) for r in rows}=={(c,d) for c in ('PCMA','PCMU') for d in (False,True)}
 assert matrix['initial_srtp_tested'] and not matrix['g0_passed'] and matrix['srtcp_tested'] and matrix['seconds_per_call']==16 and matrix['rekey_tested'] and matrix['rekey_policy']=='fresh_key_fresh_ssrc' and not matrix['peer_initiated_rekey_tested']
 for row in rows:
  assert row['transport']=='tls' and all(row[k] for k in ('ack_and_termination_verified','bye_200_verified','unregistered'))
  for m in (row['first'],row['second']):
   assert m['sent_packets']>=790 and m['received_packets']>=780
   assert math.isfinite(m['rms']) and 1000<=m['rms']<=10000 and math.isfinite(m['remote_to_own_power_ratio']) and m['remote_to_own_power_ratio']>=20
  assert row['rekey']=={'downgrade_rejected':True,'rekey_accepted':True,'peer_key_unchanged':True}
  caller=row['control']['second' if row['reverse'] else 'first']
  assert caller['peer_reports_for_rekeyed_source']>0 and caller['peer_highest_sequence_rekeyed']>65535
  for control in (row['control']['first'],row['control']['second']):
   assert control['sent_reports']>=10 and control['received_reports']>0 and control['peer_echoed_sender_reports']>0
   assert control['peer_highest_sequence']>65535 and 1<=control['authenticated_sources']<=4 and control['packet_negatives_verified']
   assert control['received_after_rekey']>=200 and 1000<=control['post_rekey_rms']<=10000 and control['post_rekey_power_ratio']>=20
 negatives=[s for s in lines if 'negative_offer' in s]
 assert len(negatives)==3 and {s['negative_offer'] for s in negatives}=={'plaintext','missing_crypto','unsupported_suite'}
 assert all(s['response']==488 and s['unregistered'] and not s['callee_invite_observed'] for s in negatives)
 tls=[s for s in lines if 'negative_tls' in s]
 assert len(tls)==2 and {s['negative_tls'] for s in tls}=={'untrusted_ca','wrong_hostname'} and all(s['rejected_before_call'] and s['typed_certificate_error_verified'] and not s['sip_endpoint_constructed'] for s in tls)
 assert counts(pbxcli('core show channels count'))==(0,0) and 'No objects found.' in pbxcli('pjsip show contacts')
 proof={'schema_version':1,'runner_image':image,'pbx_image':pbx_image,'elf_sha256':elf_hash,'runtime_mounts_and_hardening_verified':hardened,'pbx_two_active_channels_one_call_observed':observed,'mandatory_sdes_nonoptimistic_endpoints_verified':True,'internal_network_no_published_ports':True,'four_encrypted_media_calls_validated':True,'eight_endpoint_srtcp_report_exchanges_validated':True,'peer_lsr_and_rollover_reported_by_probe':True,'srtcp_tag_index_ebit_clear_replay_negatives_validated':True,'mid_call_plaintext_reoffer_488_validated':True,'fresh_key_fresh_ssrc_rekey_with_continued_media_validated':True,'three_real_488_negative_offers_validated':True,'two_real_tls_identity_negatives_validated':True,'zero_calls_channels_contacts_before_down':True,'count_parser_positive_nonzero_malformed_controls':True}
 assert hardened and observed
 save('-matrix.json',matrix);save('-negatives.json',negatives+tls)
finally:
 if process is not None and process.poll() is None:
  subprocess.run(['docker','stop','-t','5','rust-microsip-secure-call-probe'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=15)
  process.wait(timeout=15)
 run(base+['down','--volumes','--remove-orphans'])
proof['zero_project_containers_after_down']=not run(['docker','ps','-aq','--filter','label=com.docker.compose.project=rust-microsip-lab']).strip()
proof['zero_project_networks_after_down']=not run(['docker','network','ls','-q','--filter','label=com.docker.compose.project=rust-microsip-lab']).strip()
assert proof['zero_project_containers_after_down'] and proof['zero_project_networks_after_down']
save('-proof.json',proof);print(json.dumps(proof))
