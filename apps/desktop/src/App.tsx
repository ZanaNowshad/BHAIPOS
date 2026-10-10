import { FormEvent, useCallback, useEffect, useRef, useState } from 'react';
import { posApi, type CartSnapshot, type CheckoutResult, type FailedPrintJob, type HeldCart, type ManagerApprovalCredentials, type OperationalAlert, type Payment, type RefundableSale } from './api/pos';
import { adminApi, type ConfigureOfflinePolicy, type DiagnosticSnapshot, type OfflinePolicyDecision, type OfflinePolicyDetail, type OperationalDiagnostics, type RestorePreview, type RoleAdministrationCatalog, type UserSecurityAction, type UserSecurityResult } from './api/admin';
import { fils, formatBhd, parseBhd, parseQuantityMilli } from './domain/money';

type Phase = 'loading'|'setup'|'login'|'policy-recovery'|'cashier'|'admin'|'diagnostics';
type Tender = Payment['kind'];
type CashMovementKind = 'PAID_IN'|'PAID_OUT'|'SAFE_DROP'|'NO_SALE'|'PETTY_CASH';
type ManagerApprovalChallenge = {title:string;detail:string;approve:(credentials:ManagerApprovalCredentials)=>Promise<void>};

function message(error: unknown): string { return error instanceof Error ? error.message : String(error); }

function useBarcodeFocus(enabled: boolean) {
  const inputRef=useRef<HTMLInputElement>(null);
  const restoreFocus=useCallback(() => { if (enabled) window.setTimeout(() => inputRef.current?.focus(),0); },[enabled]);
  useEffect(restoreFocus,[restoreFocus]);
  return {inputRef,restoreFocus};
}

export function App() {
  const [phase,setPhase]=useState<Phase>('loading');
  const [cashierName,setCashierName]=useState('');
  const [cashSessionId,setCashSessionId]=useState<string|null>(null);
  const [canViewAlerts,setCanViewAlerts]=useState(false);
  const [canManageAlerts,setCanManageAlerts]=useState(false);
  const [canViewDiagnostics,setCanViewDiagnostics]=useState(false);
  const [canExportDiagnostics,setCanExportDiagnostics]=useState(false);
  const [canViewOfflinePolicy,setCanViewOfflinePolicy]=useState(false);
  const [canManageOfflinePolicy,setCanManageOfflinePolicy]=useState(false);
  const [canManageUserSecurity,setCanManageUserSecurity]=useState(false);
  const [canManageRoles,setCanManageRoles]=useState(false);
  const [diagnostics,setDiagnostics]=useState<OperationalDiagnostics|null>(null);
  const [offlinePolicy,setOfflinePolicy]=useState<OfflinePolicyDetail|null>(null);
  const [diagnosticSnapshots,setDiagnosticSnapshots]=useState<DiagnosticSnapshot[]>([]);
  const [restorePreview,setRestorePreview]=useState<RestorePreview|null>(null);
  const [alerts,setAlerts]=useState<OperationalAlert[]>([]);
  const [cart,setCart]=useState<CartSnapshot|null>(null);
  const [selected,setSelected]=useState<string|null>(null);
  const [scan,setScan]=useState('');
  const [tender,setTender]=useState<Tender>('Cash');
  const [busy,setBusy]=useState(false);
  const [notice,setNotice]=useState('Starting local database…');
  const [held,setHeld]=useState<HeldCart[]|null>(null);
  const [refundSale,setRefundSale]=useState<RefundableSale|null>(null);
  const [failedPrints,setFailedPrints]=useState<FailedPrintJob[]|null>(null);
  const [cashControlOpen,setCashControlOpen]=useState(false);
  const [lastSale,setLastSale]=useState<CheckoutResult|null>(null);
  const [managerChallenge,setManagerChallenge]=useState<ManagerApprovalChallenge|null>(null);
  const {inputRef,restoreFocus}=useBarcodeFocus(phase==='cashier'&&!busy&&held===null&&refundSale===null&&failedPrints===null&&!cashControlOpen&&managerChallenge===null);

  useEffect(() => {
    posApi.health().then(health => {setPhase(health.initialized?'login':'setup');setNotice(health.ready?'Offline database ready':'Database integrity check failed');}).catch(error => {setNotice(message(error));setPhase('login');});
  },[]);

  const newCart=useCallback(async () => {
    const created=await posApi.createCart();
    const snapshot=await posApi.cartSnapshot(created.id);
    setCart(snapshot); setSelected(null);
  },[]);

  async function loggedIn(displayName:string,recoveredSession:string|null,viewAlerts:boolean,manageAlerts:boolean,viewDiagnostics:boolean,exportDiagnostics:boolean,viewOfflinePolicy:boolean,manageOfflinePolicy:boolean,manageUserSecurity:boolean,manageRoles:boolean,policyRecoveryRequired:boolean){
    setCashierName(displayName); setCashSessionId(recoveredSession); setCanViewAlerts(viewAlerts); setCanManageAlerts(manageAlerts); setCanViewDiagnostics(viewDiagnostics); setCanExportDiagnostics(exportDiagnostics); setCanViewOfflinePolicy(viewOfflinePolicy); setCanManageOfflinePolicy(manageOfflinePolicy); setCanManageUserSecurity(manageUserSecurity); setCanManageRoles(manageRoles);
    if(policyRecoveryRequired){setOfflinePolicy(await adminApi.activeOfflinePolicy());setPhase('policy-recovery');setNotice('Selling is locked until a bounded offline policy is activated');return;}
    setPhase('cashier'); setNotice(recoveredSession?'Open shift recovered':'Signed in — open a shift for cash tenders');
    await newCart();
  }

  async function showDiagnostics(){
    try{const [live,history,policy]=await Promise.all([adminApi.diagnostics(),adminApi.diagnosticSnapshots(),canViewOfflinePolicy?adminApi.activeOfflinePolicy():Promise.resolve(null)]);setDiagnostics(live);setDiagnosticSnapshots(history);setOfflinePolicy(policy);setPhase('diagnostics');setNotice('Live redacted diagnostics loaded');}
    catch(error){setNotice(message(error));}
  }

  async function configureOfflinePolicy(request:Omit<ConfigureOfflinePolicy,'operationId'>){
    try{const result=await adminApi.configureOfflinePolicy({...request,operationId:crypto.randomUUID()});setOfflinePolicy(await adminApi.activeOfflinePolicy());setNotice(`Offline policy version ${result.version} activated for this branch and terminal`);}
    catch(error){setNotice(message(error));}
  }

  async function administerUserSecurity(request:{targetEmployeeNo:string;action:UserSecurityAction;newPin?:string;reason:string}):Promise<UserSecurityResult>{
    try{const result=await adminApi.administerUserSecurity({...request,operationId:crypto.randomUUID()});setNotice(`${result.display_name} · ${result.event_type.replaceAll('_',' ').toLowerCase()} · ${result.previous_status} → ${result.new_status}`);return result;}
    catch(error){setNotice(message(error));throw error;}
  }

  async function captureDiagnostics(){
    try{const receipt=await adminApi.captureDiagnostics(crypto.randomUUID());setDiagnosticSnapshots(await adminApi.diagnosticSnapshots());setNotice(`Redacted diagnostics exported · ${receipt.file_name}`);}
    catch(error){setNotice(message(error));}
  }

  async function createBackup(){
    try{const job=await adminApi.createBackup(crypto.randomUUID());setNotice(`Backup queued · ${job.job_id.slice(0,8)}`);setDiagnostics(await adminApi.diagnostics());}
    catch(error){setNotice(message(error));}
  }

  async function configureSchedule(){
    const interval=window.prompt('Backup interval in hours','24');if(interval===null)return;
    const retention=window.prompt('Verified scheduled backups to retain','7');if(retention===null)return;
    const intervalMinutes=Math.round(Number(interval)*60);const retentionCount=Number(retention);
    if(!Number.isInteger(intervalMinutes)||!Number.isInteger(retentionCount)) {setNotice('Interval and retention must be whole supported values');return;}
    try{await adminApi.configureBackupSchedule(crypto.randomUUID(),intervalMinutes,retentionCount,true,new Date().toISOString(),30);setDiagnostics(await adminApi.diagnostics());setNotice('Backup schedule configured');}
    catch(error){setNotice(message(error));}
  }

  async function previewLatestRestore(){
    if(!diagnostics?.latest_backup)return;
    try{setRestorePreview(await adminApi.previewRestore(diagnostics.latest_backup.backup_id));setNotice('Restore preview verified');}
    catch(error){setNotice(message(error));}
  }

  async function restoreLatest(){
    if(!restorePreview)return;
    const confirmation=window.prompt('Type RESTORE to replace the local database after a safety backup','');
    if(confirmation!=='RESTORE'){setNotice('Restore cancelled');return;}
    try{await adminApi.restoreBackup(crypto.randomUUID(),restorePreview.backup_id,restorePreview.sha256);setRestorePreview(null);setDiagnostics(await adminApi.diagnostics());setNotice('Verified restore completed');}
    catch(error){setNotice(message(error));}
  }

  async function showAlertCentre(){
    try{setAlerts(await posApi.listOperationalAlerts());setPhase('admin');setNotice('Operational alerts loaded');}
    catch(error){setNotice(message(error));}
  }

  async function transitionAlert(alert:OperationalAlert,newStatus:'ACKNOWLEDGED'|'IN_PROGRESS'|'RESOLVED'|'DISMISSED'){
    let note:string|undefined;
    if(newStatus==='RESOLVED'||newStatus==='DISMISSED'){
      const entered=window.prompt(`${newStatus==='RESOLVED'?'Resolution':'Dismissal'} evidence`,'');
      if(!entered?.trim()){setNotice('A closure note is required');return;}
      note=entered.trim();
    }
    try{await posApi.transitionOperationalAlert(alert.alert_id,newStatus,note);setAlerts(await posApi.listOperationalAlerts());setNotice(`Alert ${newStatus.toLowerCase().replace('_',' ')}`);}
    catch(error){setNotice(message(error));}
  }

  async function scanProduct(event:FormEvent){
    event.preventDefault(); if (!cart||!scan.trim()||busy) return;
    setBusy(true);
    try {const next=await posApi.scanBarcode(cart.cart_id,scan.trim());setCart(next);setSelected(next.lines.at(-1)?.id??null);setScan('');setNotice('Item added');}
    catch(error){setNotice(message(error));}
    finally{setBusy(false);restoreFocus();}
  }

  async function openShift(){
    const entered=window.prompt('Opening float in BHD','20.000'); if (entered===null) return;
    try {const amount=parseBhd(entered);const id=crypto.randomUUID();await posApi.openCashSession(id,amount);setCashSessionId(id);setNotice('Cash session opened');}
    catch(error){setNotice(message(error));} finally{restoreFocus();}
  }

  async function completeSale(){
    if (!cart||cart.lines.length===0||busy) return;
    setBusy(true);
    try {
      const payment:Payment={kind:tender,amountFils:cart.candidate_total};
      if (tender==='Cash') {
        if (!cashSessionId) throw new Error('Open a cash session before accepting cash');
        const entered=window.prompt('Cash received in BHD',formatBhd(fils(cart.candidate_total))); if (entered===null) return;
        payment.tenderedFils=parseBhd(entered);
      } else if (tender==='BenefitPay'||tender==='BankTransfer'||tender==='Card') {
        const reference=window.prompt(`${tender} reference (optional)`,''); if (reference?.trim()) payment.reference=reference.trim();
      }
      const committed=await posApi.checkout(cart.cart_id,crypto.randomUUID(),[payment]);
      setLastSale(committed); setNotice(`Sale committed · ${committed.receipt_number} · change ${formatBhd(fils(committed.change))} BHD`);
      await newCart();
    } catch(error){setNotice(message(error));}
    finally{setBusy(false);restoreFocus();}
  }

  async function holdCurrent(){
    if (!cart||cart.lines.length===0) return;
    const note=window.prompt('Held-cart note','')??undefined;
    try {await posApi.holdCart(cart.cart_id,note);await newCart();setNotice('Cart held');}
    catch(error){setNotice(message(error));} finally{restoreFocus();}
  }

  async function showHeld(){
    try{setHeld(await posApi.listHeldCarts());}catch(error){setNotice(message(error));restoreFocus();}
  }

  async function restoreHeld(cartId:string){
    try{setCart(await posApi.restoreCart(cartId));setHeld(null);setNotice('Held cart restored');}
    catch(error){setNotice(message(error));}finally{restoreFocus();}
  }

  async function showRefund(){
    const receiptNumber=window.prompt('Original receipt number',''); if (!receiptNumber?.trim()) return;
    setBusy(true);
    try{setRefundSale(await posApi.findRefundableSale(receiptNumber.trim()));setNotice('Select a refundable line');}
    catch(error){setNotice(message(error));restoreFocus();}
    finally{setBusy(false);}
  }

  async function recordCashMovement(kind:CashMovementKind,amountFils:number,reason:string){
    const request={operationId:crypto.randomUUID(),kind,amountFils,reason:reason.trim()||undefined};
    const label=kind.replaceAll('_',' ').toLowerCase();
    try{
      await posApi.recordCashMovement(request);
      setCashControlOpen(false);setNotice(`${label} recorded with immutable cash evidence`);
    }catch(error){
      const detail=message(error);
      if(!detail.includes('manager approval is required'))throw error;
      setManagerChallenge({title:`Approve ${label}`,detail:`Approve ${kind==='NO_SALE'?'a no-sale drawer opening':`${formatBhd(fils(amountFils))} BHD`} for this exact cash event. This approval expires in two minutes.`,approve:async credentials=>{await posApi.recordCashMovement({...request,managerApproval:credentials});setManagerChallenge(null);setCashControlOpen(false);setNotice(`Manager-approved ${label} recorded`);}});
    }
  }

  async function refundLine(sale:RefundableSale,line:RefundableSale['lines'][number]){
    const entered=window.prompt(`Return quantity for ${line.product_name}`,(line.remaining_quantity/1000).toFixed(3)); if (entered===null) return;
    try{
      const quantityMilli=parseQuantityMilli(entered);
      if (quantityMilli>line.remaining_quantity) throw new Error('Quantity exceeds the remaining refundable quantity');
      const lines=[{saleLineId:line.sale_line_id,quantityMilli}];
      const quote=await posApi.quoteRefund(sale.sale_id,lines);
      if (!window.confirm(`Refund ${formatBhd(fils(quote.total))} BHD via ${tender}?`)) return;
      if (tender==='Cash'&&!cashSessionId) throw new Error('Open a cash session before issuing a cash refund');
      const reason=window.prompt('Refund reason','Customer return'); if (!reason?.trim()) throw new Error('A refund reason is required');
      const payments:Payment[]=quote.total===0?[]:[{kind:tender,amountFils:quote.total}];
      const request={operationId:crypto.randomUUID(),saleId:sale.sale_id,reason:reason.trim(),lines,payments};
      try{
        const committed=await posApi.refund(request);
        setRefundSale(null);setNotice(`Refund committed · ${formatBhd(fils(committed.total))} BHD`);
      }catch(error){
        const detail=message(error);
        if(!detail.includes('manager approval is required'))throw error;
        setManagerChallenge({title:'Approve refund',detail:`Approve ${formatBhd(fils(quote.total))} BHD for receipt ${sale.receipt_number}. This approval applies only to this exact refund and expires in two minutes.`,approve:async credentials=>{const committed=await posApi.refund({...request,managerApproval:credentials});setManagerChallenge(null);setRefundSale(null);setNotice(`Manager-approved refund committed · ${formatBhd(fils(committed.total))} BHD`);}});
      }
    }catch(error){setNotice(message(error));}finally{restoreFocus();}
  }

  async function showPrintRecovery(){
    try{setFailedPrints(await posApi.listFailedPrintJobs());setNotice('Failed print queue loaded');}
    catch(error){setNotice(message(error));restoreFocus();}
  }

  async function retryPrint(printJobId:string){
    try{await posApi.requeueFailedPrintJob(printJobId);setFailedPrints(await posApi.listFailedPrintJobs());setNotice('Receipt returned to the print queue');}
    catch(error){setNotice(message(error));}
  }

  async function lock(){await posApi.logout();setCart(null);setCashSessionId(null);setCanViewAlerts(false);setCanManageAlerts(false);setCanViewDiagnostics(false);setCanExportDiagnostics(false);setCanViewOfflinePolicy(false);setCanManageOfflinePolicy(false);setCanManageUserSecurity(false);setCanManageRoles(false);setAlerts([]);setDiagnostics(null);setOfflinePolicy(null);setDiagnosticSnapshots([]);setRestorePreview(null);setPhase('login');setNotice('Terminal locked');}

  if (phase==='loading') return <StatusScreen title="BHAIPOS" detail={notice}/>;
  if (phase==='setup') return <SetupScreen onReady={() => {setPhase('login');setNotice('Business initialized — sign in offline');}} setNotice={setNotice}/>;
  if (phase==='login') return <LoginScreen notice={notice} onLogin={loggedIn} setNotice={setNotice}/>;
  if (phase==='policy-recovery') return <OfflinePolicyRecoveryScreen policy={offlinePolicy} notice={notice} onConfigure={configureOfflinePolicy} onLock={lock}/>;
  if (phase==='admin') return <AlertCentre alerts={alerts} canManage={canManageAlerts} notice={notice} onTransition={transitionAlert} onRefresh={showAlertCentre} onCashier={()=>{setPhase('cashier');restoreFocus();}} onLock={lock}/>;
  if (phase==='diagnostics'&&diagnostics) return <DiagnosticsCentre diagnostics={diagnostics} snapshots={diagnosticSnapshots} preview={restorePreview} offlinePolicy={offlinePolicy} notice={notice} canViewAlerts={canViewAlerts} canExport={canExportDiagnostics} canViewOfflinePolicy={canViewOfflinePolicy} canManageOfflinePolicy={canManageOfflinePolicy} canManageUserSecurity={canManageUserSecurity} canManageRoles={canManageRoles} onAlerts={showAlertCentre} onRefresh={showDiagnostics} onCapture={captureDiagnostics} onConfigureOfflinePolicy={configureOfflinePolicy} onAdministerUserSecurity={administerUserSecurity} onNotice={setNotice} onBackup={createBackup} onSchedule={configureSchedule} onPreview={previewLatestRestore} onRestore={restoreLatest} onCashier={()=>{setPhase('cashier');restoreFocus();}} onLock={lock}/>;
  if (!cart) return <StatusScreen title="Preparing cashier" detail={notice}/>;

  return <div className="app-shell">
    <header className="topbar">
      <div><strong>BHAIPOS</strong><span className="branch">Local terminal · schema 0027</span></div>
      <div className="status"><span className="dot"/>{notice}</div>
      <div>{canViewAlerts&&<button onClick={showAlertCentre}>Alert Centre</button>}{canViewDiagnostics&&<button onClick={showDiagnostics}>Diagnostics</button>}<button onClick={cashSessionId?undefined:openShift}>{cashSessionId?'Shift Open':'Open Shift'}</button><button onClick={lock}>Lock</button></div>
    </header>
    <main className="cashier-grid">
      <section className="selling-panel">
        <form className="scan-row" onSubmit={scanProduct}><input ref={inputRef} value={scan} onChange={event=>setScan(event.target.value)} aria-label="Scan barcode" placeholder="Scan barcode, SKU or PLU…" autoComplete="off"/><button className="primary" disabled={busy}>Add</button></form>
        <div className="cart-meta"><span>Cart {cart.cart_id.slice(0,8)}</span><span>Walk-in customer</span><span>{cart.lines.length} lines</span></div>
        <div className="line-header"><span>Item</span><span>Qty</span><span>Price</span><span>Total</span></div>
        <div className="lines">{cart.lines.length===0?<div className="empty-cart">Scan an item to begin</div>:cart.lines.map(line=><button key={line.id} className={`line ${selected===line.id?'selected':''}`} onClick={()=>{setSelected(line.id);restoreFocus();}}>
          <span className="product"><b>{line.name}</b><small>{line.barcode??line.sku}</small></span><span>{(line.quantity/1000).toFixed(3)}</span><span>{formatBhd(fils(line.unit_price))}</span><strong>{formatBhd(fils(line.gross))}</strong>
        </button>)}</div>
        <div className="action-grid"><button disabled>Qty</button><button disabled>Discount</button><button disabled>Price Override</button><button disabled>Remove</button><button disabled>Customer</button><button disabled>Note</button><button onClick={holdCurrent}>Hold Cart</button><button onClick={showHeld}>Held Carts</button><button onClick={showRefund}>Refund</button><button disabled>Delivery</button><button onClick={showPrintRecovery}>Reprint</button><button disabled={!cashSessionId} onClick={()=>setCashControlOpen(true)}>Cash Control</button></div>
      </section>
      <aside className="checkout-panel">
        <div className="summary"><div><span>Net candidate</span><b>{formatBhd(fils(cart.candidate_subtotal))} BHD</b></div><div><span>VAT candidate</span><b>{formatBhd(fils(cart.candidate_tax))} BHD</b></div><div className="grand"><span>Cart estimate</span><strong>{formatBhd(fils(cart.candidate_total))} BHD</strong></div></div>
        <p className="recalc-note">Final totals are recalculated by the trusted local service at commit.</p>
        <div className="tender-grid">{([['Cash','Cash'],['Card','Card'],['BenefitPay','BenefitPay'],['BankTransfer','Bank'],['CustomerCredit','Credit']] as [Tender,string][]).map(([value,label])=><button key={value} className={tender===value?'selected-tender':''} onClick={()=>{setTender(value);restoreFocus();}}>{label}</button>)}</div>
        <button className="pay" disabled={busy||cart.lines.length===0} onClick={completeSale}>COMMIT SALE</button>
        {lastSale&&<div className="last-sale"><span>Last receipt</span><b>{lastSale.receipt_number}</b><small>Committed {formatBhd(fils(lastSale.total))} BHD</small></div>}
        <div className="shift"><div><span>Cashier</span><b>{cashierName}</b></div><div><span>Shift</span><b>{cashSessionId?'Open':'Not open'}</b></div><div><span>Mode</span><b>Offline authority</b></div></div>
      </aside>
    </main>
    {held!==null&&<div className="modal-backdrop"><section className="shift-modal" role="dialog" aria-modal="true" aria-label="Held carts"><header><h2>Held Carts</h2><button onClick={()=>{setHeld(null);restoreFocus();}}>Close</button></header><div className="held-list">{held.length===0?<p>No held carts</p>:held.map(item=><button key={item.cart_id} onClick={()=>restoreHeld(item.cart_id)}><b>{item.note||'Untitled cart'}</b><small>{item.updated_at}</small></button>)}</div></section></div>}
    {cashControlOpen&&<CashControlModal onSubmit={recordCashMovement} onCancel={()=>{setCashControlOpen(false);restoreFocus();}} onError={setNotice}/>}
    {refundSale&&<div className="modal-backdrop"><section className="shift-modal" role="dialog" aria-modal="true" aria-label="Refund sale"><header><div><span className="eyebrow">Original receipt</span><h2>{refundSale.receipt_number}</h2></div><button onClick={()=>{setRefundSale(null);restoreFocus();}}>Close</button></header><p className="recalc-note">Values come from the immutable sale snapshot. Select one line for this refund.</p><div className="recovery-list">{refundSale.lines.filter(line=>line.remaining_quantity>0).map(line=><button key={line.sale_line_id} onClick={()=>refundLine(refundSale,line)}><span><b>{line.product_name}</b><small>Remaining {(line.remaining_quantity/1000).toFixed(3)}</small></span><strong>{formatBhd(fils(line.refundable_total))} BHD</strong></button>)}</div></section></div>}
    {managerChallenge&&<ManagerApprovalModal challenge={managerChallenge} onCancel={()=>{setManagerChallenge(null);restoreFocus();}} onError={setNotice}/>}
    {failedPrints!==null&&<div className="modal-backdrop"><section className="shift-modal" role="dialog" aria-modal="true" aria-label="Print recovery"><header><div><span className="eyebrow">Asynchronous receipt printing</span><h2>Failed Prints</h2></div><button onClick={()=>{setFailedPrints(null);restoreFocus();}}>Close</button></header><div className="recovery-list">{failedPrints.length===0?<p>No failed print jobs</p>:failedPrints.map(job=><button key={job.print_job_id} onClick={()=>retryPrint(job.print_job_id)}><span><b>{job.receipt_number??job.document_type}</b><small>{job.last_error} · attempt {job.attempts}</small></span><strong>Retry</strong></button>)}</div></section></div>}
  </div>;
}

function ManagerApprovalModal({challenge,onCancel,onError}:{challenge:ManagerApprovalChallenge;onCancel:()=>void;onError:(value:string)=>void}){
  const [employeeNo,setEmployeeNo]=useState('');
  const [pin,setPin]=useState('');
  const [busy,setBusy]=useState(false);
  async function submit(event:FormEvent){event.preventDefault();setBusy(true);try{await challenge.approve({employeeNo,pin});}catch(error){setPin('');onError(message(error));}finally{setBusy(false);}}
  return <div className="modal-backdrop"><section className="shift-modal manager-approval-modal" role="dialog" aria-modal="true" aria-label={challenge.title}><header><div><span className="eyebrow">Action-bound single-use authority</span><h2>{challenge.title}</h2></div><button disabled={busy} onClick={onCancel}>Cancel</button></header><p>{challenge.detail}</p><form onSubmit={submit}><label>Manager employee number<input value={employeeNo} onChange={event=>setEmployeeNo(event.target.value)} autoFocus autoComplete="username" required/></label><label>Manager PIN<input value={pin} onChange={event=>setPin(event.target.value)} type="password" inputMode="numeric" autoComplete="off" required/></label><button className="primary" disabled={busy}>{busy?'Verifying and committing…':'Approve exact action'}</button></form><small>The PIN is verified inside the trusted local service and is not stored in the approval record.</small></section></div>;
}

function CashControlModal({onSubmit,onCancel,onError}:{onSubmit:(kind:CashMovementKind,amountFils:number,reason:string)=>Promise<void>;onCancel:()=>void;onError:(value:string)=>void}){
  const [kind,setKind]=useState<CashMovementKind>('PAID_IN');
  const [amount,setAmount]=useState('');
  const [reason,setReason]=useState('');
  const [busy,setBusy]=useState(false);
  async function submit(event:FormEvent){event.preventDefault();setBusy(true);try{const amountFils=kind==='NO_SALE'?0:parseBhd(amount);if(kind!=='NO_SALE'&&amountFils<=0)throw new Error('Cash movement amount must be positive');if(kind!=='NO_SALE'&&!reason.trim())throw new Error('A reason is required');await onSubmit(kind,amountFils,reason);}catch(error){onError(message(error));}finally{setBusy(false);}}
  return <div className="modal-backdrop"><section className="shift-modal cash-control-modal" role="dialog" aria-modal="true" aria-label="Cash control"><header><div><span className="eyebrow">Open cash session</span><h2>Cash Control</h2></div><button disabled={busy} onClick={onCancel}>Cancel</button></header><form onSubmit={submit}><label>Movement<select value={kind} onChange={event=>setKind(event.target.value as CashMovementKind)}><option value="PAID_IN">Paid in</option><option value="PAID_OUT">Paid out</option><option value="SAFE_DROP">Safe drop</option><option value="PETTY_CASH">Petty cash</option><option value="NO_SALE">No-sale drawer open</option></select></label>{kind!=='NO_SALE'&&<label>Amount (BHD)<input value={amount} onChange={event=>setAmount(event.target.value)} inputMode="decimal" placeholder="0.000" autoFocus required/></label>}<label>Reason<input value={reason} onChange={event=>setReason(event.target.value)} placeholder={kind==='NO_SALE'?'Optional note':'Required operational reason'} required={kind!=='NO_SALE'}/></label><button className="primary" disabled={busy}>{busy?'Recording…':'Record cash event'}</button></form><small>Cash events are immutable, idempotent, attributed to this user and terminal, and included in shift reconciliation.</small></section></div>;
}

function StatusScreen({title,detail}:{title:string;detail:string}){return <main className="auth-screen"><section><h1>{title}</h1><p>{detail}</p></section></main>;}

function LoginScreen({notice,onLogin,setNotice}:{notice:string;onLogin:(name:string,session:string|null,viewAlerts:boolean,manageAlerts:boolean,viewDiagnostics:boolean,exportDiagnostics:boolean,viewOfflinePolicy:boolean,manageOfflinePolicy:boolean,manageUserSecurity:boolean,manageRoles:boolean,policyRecoveryRequired:boolean)=>Promise<void>;setNotice:(value:string)=>void}){
  const [employeeNo,setEmployeeNo]=useState('');const [pin,setPin]=useState('');const [busy,setBusy]=useState(false);
  async function submit(event:FormEvent){event.preventDefault();setBusy(true);try{const result=await posApi.login({employeeNo,pin});await onLogin(result.displayName,result.cashSessionId,result.canViewAlerts,result.canManageAlerts,result.canViewDiagnostics,result.canExportDiagnostics,result.canViewOfflinePolicy,result.canManageOfflinePolicy,result.canManageUserSecurity,result.canManageRoles,result.offlinePolicyRecoveryRequired);}catch(error){setNotice(message(error));}finally{setBusy(false);}}
  return <main className="auth-screen"><form onSubmit={submit}><h1>BHAIPOS</h1><p>{notice}</p><label>Employee number<input value={employeeNo} onChange={event=>setEmployeeNo(event.target.value)} autoFocus required/></label><label>PIN<input value={pin} onChange={event=>setPin(event.target.value)} type="password" inputMode="numeric" required/></label><button className="primary" disabled={busy}>Sign in offline</button></form></main>;
}

function AlertCentre({alerts,canManage,notice,onTransition,onRefresh,onCashier,onLock}:{alerts:OperationalAlert[];canManage:boolean;notice:string;onTransition:(alert:OperationalAlert,status:'ACKNOWLEDGED'|'IN_PROGRESS'|'RESOLVED'|'DISMISSED')=>Promise<void>;onRefresh:()=>Promise<void>;onCashier:()=>void;onLock:()=>Promise<void>}){
  const action=(alert:OperationalAlert,status:'ACKNOWLEDGED'|'IN_PROGRESS'|'RESOLVED'|'DISMISSED',label:string)=><button onClick={()=>onTransition(alert,status)}>{label}</button>;
  return <div className="admin-shell">
    <nav className="admin-nav"><h2>BHAIPOS</h2><button className="cashier-return" onClick={onCashier}>Return to Cashier</button><button className="active">Alert Centre</button><button onClick={onRefresh}>Refresh</button><button onClick={onLock}>Lock Terminal</button></nav>
    <main className="admin-main"><header><div><span className="eyebrow">Durable operational evidence</span><h1>Alert Centre</h1><p>{notice}</p></div><span className="badge">{alerts.length} active</span></header>
      <section className="alert-list">{alerts.length===0?<article className="alert-empty"><h2>No active alerts</h2><p>Resolved and dismissed alerts remain preserved in immutable history.</p></article>:alerts.map(alert=><article className={`alert-card severity-${alert.severity.toLowerCase()}`} key={alert.alert_id}>
        <div className="alert-copy"><div><span className="alert-severity">{alert.severity}</span><span className="alert-type">{alert.alert_type.replaceAll('_',' ')}</span></div><h2>{alert.title}</h2><p>{alert.status.replace('_',' ')} · {new Date(alert.created_at).toLocaleString()}</p></div>
        {canManage&&<div className="alert-actions">{alert.status==='NEW'&&action(alert,'ACKNOWLEDGED','Acknowledge')}{alert.status==='ACKNOWLEDGED'&&<>{action(alert,'IN_PROGRESS','Investigate')}{action(alert,'DISMISSED','Dismiss')}</>}{alert.status==='IN_PROGRESS'&&<>{action(alert,'RESOLVED','Resolve')}{action(alert,'DISMISSED','Dismiss')}</>}</div>}
      </article>)}</section>
    </main>
  </div>;
}

function OfflinePolicyRecoveryScreen({policy,notice,onConfigure,onLock}:{policy:OfflinePolicyDetail|null;notice:string;onConfigure:(request:Omit<ConfigureOfflinePolicy,'operationId'>)=>Promise<void>;onLock:()=>Promise<void>}){
  return <div className="admin-shell">
    <nav className="admin-nav"><h2>BHAIPOS</h2><button className="active">Policy Recovery</button><button onClick={onLock}>Lock Terminal</button></nav>
    <main className="admin-main"><header><div><span className="eyebrow">Restricted owner/administrator session</span><h1>Offline authority recovery</h1><p>{notice}</p></div><span className="badge policy-review">SELLING LOCKED</span></header>
      <section className="recovery-warning"><h2>No operational command is available in this session</h2><p>The verified PIN established identity only. Activate a bounded immutable policy below, then lock the terminal and sign in again. An explicit policy denial or revoked device cannot enter this recovery path.</p></section>
      <section className="policy-section recovery-policy"><article><h2>Current evidence</h2>{policy?<dl className="diagnostic-list"><dt>Policy version</dt><dd>{policy.version}</dd><dt>State</dt><dd>{policy.state}</dd><dt>Device checkpoint</dt><dd>{policy.device_state??'MISSING'}</dd><dt>Synchronized</dt><dd>{policy.synchronized_at?new Date(policy.synchronized_at).toLocaleString():'never'}</dd><dt>Expires</dt><dd>{new Date(policy.valid_until).toLocaleString()}</dd></dl>:<p>No active branch policy exists.</p>}<button onClick={onLock}>Finish and sign in again</button></article><OfflinePolicyEditor policy={policy} onConfigure={onConfigure}/></section>
    </main>
  </div>;
}

function DiagnosticsCentre({diagnostics,snapshots,preview,offlinePolicy,notice,canViewAlerts,canExport,canViewOfflinePolicy,canManageOfflinePolicy,canManageUserSecurity,canManageRoles,onAlerts,onRefresh,onCapture,onConfigureOfflinePolicy,onAdministerUserSecurity,onNotice,onBackup,onSchedule,onPreview,onRestore,onCashier,onLock}:{diagnostics:OperationalDiagnostics;snapshots:DiagnosticSnapshot[];preview:RestorePreview|null;offlinePolicy:OfflinePolicyDetail|null;notice:string;canViewAlerts:boolean;canExport:boolean;canViewOfflinePolicy:boolean;canManageOfflinePolicy:boolean;canManageUserSecurity:boolean;canManageRoles:boolean;onAlerts:()=>Promise<void>;onRefresh:()=>Promise<void>;onCapture:()=>Promise<void>;onConfigureOfflinePolicy:(request:Omit<ConfigureOfflinePolicy,'operationId'>)=>Promise<void>;onAdministerUserSecurity:(request:{targetEmployeeNo:string;action:UserSecurityAction;newPin?:string;reason:string})=>Promise<UserSecurityResult>;onNotice:(value:string)=>void;onBackup:()=>Promise<void>;onSchedule:()=>Promise<void>;onPreview:()=>Promise<void>;onRestore:()=>Promise<void>;onCashier:()=>void;onLock:()=>Promise<void>}){
  const healthy=diagnostics.database_integrity==='ok'&&diagnostics.foreign_key_violations===0;
  const policyState=offlinePolicyEvidenceState(offlinePolicy);
  return <div className="admin-shell">
    <nav className="admin-nav"><h2>BHAIPOS</h2><button className="cashier-return" onClick={onCashier}>Return to Cashier</button>{canViewAlerts&&<button onClick={onAlerts}>Alert Centre</button>}<button className="active">Diagnostics</button><button onClick={onRefresh}>Refresh</button><button onClick={onLock}>Lock Terminal</button></nav>
    <main className="admin-main"><header><div><span className="eyebrow">Redacted trusted-terminal evidence</span><h1>Diagnostics & Backups</h1><p>{notice}</p></div><span className="badge">{healthy?'Database healthy':'Review required'}</span></header>
      <section className="metric-grid"><article><span>Schema</span><strong>{diagnostics.schema_version}</strong></article><article><span>Device</span><strong>{diagnostics.device_status}</strong></article><article><span>Pending jobs</span><strong>{diagnostics.pending_background_jobs}</strong></article><article><span>Sync review</span><strong>{diagnostics.sync_requires_review}</strong></article></section>
      <section className="admin-panels"><article><h2>Database</h2><dl className="diagnostic-list"><dt>Integrity</dt><dd>{diagnostics.database_integrity}</dd><dt>Foreign-key violations</dt><dd>{diagnostics.foreign_key_violations}</dd><dt>Path</dt><dd>{diagnostics.database_path}</dd><dt>Build</dt><dd>{diagnostics.build_sha??'development build'}</dd><dt>Application</dt><dd>{diagnostics.application_version}</dd></dl></article><article><h2>Terminal</h2><dl className="diagnostic-list"><dt>Device ID</dt><dd>{diagnostics.device_id}</dd><dt>Register ID</dt><dd>{diagnostics.register_id}</dd><dt>Last heartbeat</dt><dd>{diagnostics.last_heartbeat_at??'not reported'}</dd><dt>Printer profile</dt><dd>{diagnostics.printer_configured?'configured':'missing'}</dd><dt>Printer probe</dt><dd>{diagnostics.printer_state} · {diagnostics.printer_detail}</dd><dt>Failed prints</dt><dd>{diagnostics.failed_print_jobs}</dd><dt>Hub mode</dt><dd>{diagnostics.hub_mode}</dd></dl></article></section>
      <section className="admin-panels"><article><h2>Backup authority</h2><p>{diagnostics.backup_schedule?`${diagnostics.backup_schedule.state} · next ${diagnostics.backup_schedule.next_run_at} · retain ${diagnostics.backup_schedule.retention_count}`:'No scheduled backup configured'}</p><div className="admin-actions"><button className="primary" onClick={onBackup}>Create verified backup</button><button onClick={onSchedule}>Configure schedule</button></div><small>Trusted directory: {diagnostics.backup_directory}</small></article><article><h2>Latest verified evidence</h2>{diagnostics.latest_backup?<><p>{diagnostics.latest_backup.backup_type} · {diagnostics.latest_backup.state} · {diagnostics.latest_backup.integrity_state??'unknown integrity'}</p><small>{diagnostics.latest_backup.created_at} · {diagnostics.latest_backup.backup_id}</small><div className="admin-actions"><button onClick={onPreview}>Verify restore preview</button>{preview?.compatible&&<button className="danger" onClick={onRestore}>Restore after safety backup</button>}</div>{preview&&<p>SHA-256 {preview.sha256} · {preview.byte_size} bytes · schema {preview.schema_version}</p>}</>:<p>No backup evidence exists yet.</p>}</article></section>
      <section className="admin-panels"><article><h2>Synchronization</h2><dl className="diagnostic-list"><dt>Pending mutations</dt><dd>{diagnostics.pending_sync_mutations}</dd><dt>Requires review</dt><dd>{diagnostics.sync_requires_review}</dd><dt>Last checkpoint</dt><dd>{diagnostics.last_sync_at??'never'}</dd></dl></article><article><h2>Optional modules</h2><dl className="diagnostic-list"><dt>WhatsApp</dt><dd>{diagnostics.whatsapp_status}</dd><dt>OCR</dt><dd>{diagnostics.ocr_status}</dd><dt>Jobs requiring review</dt><dd>{diagnostics.background_jobs_requires_review}</dd></dl></article></section>
      {canViewOfflinePolicy&&<section className="policy-section"><article><div className="policy-heading"><div><span className="eyebrow">Bounded disconnected authority</span><h2>Offline policy</h2></div><span className={`badge policy-${policyState.toLowerCase()}`}>{policyState}</span></div>{offlinePolicy?<><dl className="diagnostic-list"><dt>Immutable version</dt><dd>{offlinePolicy.version}</dd><dt>Branch policy state</dt><dd>{offlinePolicy.state}</dd><dt>Device checkpoint</dt><dd>{offlinePolicy.device_state??'MISSING'}</dd><dt>Synchronized</dt><dd>{offlinePolicy.synchronized_at?new Date(offlinePolicy.synchronized_at).toLocaleString():'never'}</dd><dt>Policy expires</dt><dd>{new Date(offlinePolicy.valid_until).toLocaleString()}</dd><dt>Device authorization expires</dt><dd>{offlinePolicy.device_valid_until?new Date(offlinePolicy.device_valid_until).toLocaleString():'not authorized'}</dd></dl><div className="policy-rules">{offlinePolicy.rules.map(rule=><div key={rule.action_type}><b>{rule.action_type.replaceAll('_',' ')}</b><span>{rule.decision.replaceAll('_',' ')}</span><small>{rule.max_offline_age_minutes===0?'No offline age allowed':`${rule.max_offline_age_minutes} minute maximum age`}</small></div>)}</div></>:<p>No active offline policy exists for this branch. Offline authorization evaluation fails closed until an authorized administrator activates one.</p>}</article>{canManageOfflinePolicy&&<OfflinePolicyEditor policy={offlinePolicy} onConfigure={onConfigureOfflinePolicy}/>}</section>}
      {canManageUserSecurity&&<section className="admin-panels"><UserSecurityPanel onSubmit={onAdministerUserSecurity}/><article><h2>Credential recovery controls</h2><p>Each action is branch-scoped, device-attributed, idempotent, and preserved in immutable security and hash-chained audit evidence.</p><small>PIN resets do not unlock a locked account. Use the separate unlock action after reviewing the lockout evidence.</small></article></section>}
      {canManageRoles&&<RoleAdministrationPanel onNotice={onNotice}/>}
      <section className="admin-panels"><article><h2>Redacted diagnostic evidence</h2><p>Exports contain scoped operational evidence without database paths, backup paths, PINs, credentials, cookies, or provider secrets.</p>{canExport?<button className="primary" onClick={onCapture}>Capture and export diagnostics</button>:<small>Export permission is not assigned.</small>}</article><article><h2>Immutable snapshot history</h2>{snapshots.length===0?<p>No diagnostic snapshots captured.</p>:<div className="snapshot-list">{snapshots.map(snapshot=><div key={snapshot.snapshot_id}><b>{new Date(snapshot.created_at).toLocaleString()}</b><small>{snapshot.app_version} · schema {snapshot.schema_version} · SHA-256 {snapshot.payload_sha256}</small></div>)}</div>}</article></section>
    </main>
  </div>;
}

function UserSecurityPanel({onSubmit}:{onSubmit:(request:{targetEmployeeNo:string;action:UserSecurityAction;newPin?:string;reason:string})=>Promise<UserSecurityResult>}){
  const [targetEmployeeNo,setTargetEmployeeNo]=useState('');
  const [action,setAction]=useState<UserSecurityAction>('RESET_PIN');
  const [newPin,setNewPin]=useState('');
  const [reason,setReason]=useState('');
  const [busy,setBusy]=useState(false);
  async function submit(event:FormEvent){
    event.preventDefault();setBusy(true);
    try{
      await onSubmit({targetEmployeeNo:targetEmployeeNo.trim(),action,newPin:action==='RESET_PIN'?newPin:undefined,reason:reason.trim()});
      setNewPin('');setReason('');
    }catch{}finally{setBusy(false);}
  }
  return <article><span className="eyebrow">Owner or administrator</span><h2>Staff account recovery</h2><form className="policy-editor" onSubmit={submit}><label>Employee number<input value={targetEmployeeNo} onChange={event=>setTargetEmployeeNo(event.target.value)} autoComplete="off" required/></label><label>Security action<select value={action} onChange={event=>{setAction(event.target.value as UserSecurityAction);setNewPin('');}}><option value="RESET_PIN">Reset PIN</option><option value="UNLOCK">Unlock account</option><option value="SUSPEND">Suspend account</option><option value="REACTIVATE">Reactivate account</option></select></label>{action==='RESET_PIN'&&<label>Replacement PIN<input type="password" inputMode="numeric" autoComplete="new-password" value={newPin} onChange={event=>setNewPin(event.target.value)} required/></label>}<label>Reason<input value={reason} onChange={event=>setReason(event.target.value)} minLength={3} required/></label><button className={action==='SUSPEND'?'danger':'primary'} disabled={busy}>{busy?'Applying…':'Apply auditable action'}</button></form></article>;
}

function RoleAdministrationPanel({onNotice}:{onNotice:(value:string)=>void}){
  const [catalog,setCatalog]=useState<RoleAdministrationCatalog|null>(null);
  const [roleName,setRoleName]=useState('');
  const [permissionCodes,setPermissionCodes]=useState<string[]>([]);
  const [reason,setReason]=useState('');
  const [employeeNo,setEmployeeNo]=useState('');
  const [assigned,setAssigned]=useState(true);
  const [busy,setBusy]=useState(false);
  const refresh=useCallback(async()=>{try{setCatalog(await adminApi.roleAdministrationCatalog());}catch(error){onNotice(message(error));}},[onNotice]);
  useEffect(()=>{void refresh();},[refresh]);
  function chooseRole(name:string){setRoleName(name);const existing=catalog?.custom_roles.find(role=>role[1]===name);setPermissionCodes(existing?.[3]??[]);}
  function togglePermission(code:string){setPermissionCodes(current=>current.includes(code)?current.filter(value=>value!==code):[...current,code]);}
  async function configure(event:FormEvent){event.preventDefault();setBusy(true);try{const result=await adminApi.configureCustomRole({operationId:crypto.randomUUID(),roleName:roleName.trim(),permissionCodes,reason:reason.trim()});onNotice(`${result.role_name} version ${result.version} saved with ${result.permission_codes.length} permissions`);setReason('');await refresh();}catch(error){onNotice(message(error));}finally{setBusy(false);}}
  async function assign(event:FormEvent){event.preventDefault();setBusy(true);try{const result=await adminApi.assignCustomRole({operationId:crypto.randomUUID(),targetEmployeeNo:employeeNo.trim(),roleName:roleName.trim(),assigned,reason:reason.trim()});onNotice(`${result.role_name} ${assigned?'assigned to':'removed from'} ${employeeNo.trim()}`);setEmployeeNo('');setReason('');}catch(error){onNotice(message(error));}finally{setBusy(false);}}
  return <section className="policy-section"><article><span className="eyebrow">Fail-closed RBAC</span><h2>Custom role permissions</h2><form className="policy-editor" onSubmit={configure}><label>Custom role<select value={roleName} onChange={event=>chooseRole(event.target.value)} required><option value="">Select or create below</option>{catalog?.custom_roles.map(role=><option key={role[0]} value={role[1]}>{role[1]} · v{role[2]}</option>)}</select></label><label>New custom role name<input value={roleName} onChange={event=>chooseRole(event.target.value)} minLength={2} required/></label><div className="policy-rules">{catalog?.permissions.map(([code,description])=><label key={code}><input type="checkbox" checked={permissionCodes.includes(code)} onChange={()=>togglePermission(code)}/><b>{code}</b><small>{description}</small></label>)}</div><label>Change reason<input value={reason} onChange={event=>setReason(event.target.value)} minLength={3} required/></label><button className="primary" disabled={busy||!catalog}>{busy?'Saving…':'Save immutable role version'}</button></form></article><article><span className="eyebrow">Current branch only</span><h2>Assign custom role</h2><form className="policy-editor" onSubmit={assign}><label>Employee number<input value={employeeNo} onChange={event=>setEmployeeNo(event.target.value)} required/></label><label>Custom role<select value={roleName} onChange={event=>chooseRole(event.target.value)} required><option value="">Select role</option>{catalog?.custom_roles.map(role=><option key={role[0]} value={role[1]}>{role[1]}</option>)}</select></label><label>Assignment<select value={assigned?'ASSIGN':'UNASSIGN'} onChange={event=>setAssigned(event.target.value==='ASSIGN')}><option value="ASSIGN">Assign</option><option value="UNASSIGN">Remove</option></select></label><label>Reason<input value={reason} onChange={event=>setReason(event.target.value)} minLength={3} required/></label><button className="primary" disabled={busy||!catalog}>{busy?'Applying…':'Apply branch assignment'}</button></form><small>Built-in roles are not editable here. Every permission replacement and assignment is idempotent, device-attributed and audit chained.</small></article></section>;
}

const OFFLINE_ACTIONS: Array<{actionType:string;decision:OfflinePolicyDecision;maxOfflineAgeMinutes:number}> = [
  {actionType:'LOGIN',decision:'ALLOW',maxOfflineAgeMinutes:1440},
  {actionType:'SALE_CHECKOUT',decision:'ALLOW',maxOfflineAgeMinutes:1440},
  {actionType:'CASH_EVENT',decision:'ALLOW',maxOfflineAgeMinutes:1440},
  {actionType:'REFUND',decision:'REQUIRE_MANAGER_APPROVAL',maxOfflineAgeMinutes:120},
  {actionType:'VOID',decision:'REQUIRE_MANAGER_APPROVAL',maxOfflineAgeMinutes:120},
  {actionType:'PRICE_OVERRIDE',decision:'REQUIRE_MANAGER_APPROVAL',maxOfflineAgeMinutes:120},
  {actionType:'CUSTOMER_CREDIT',decision:'DENY',maxOfflineAgeMinutes:0},
  {actionType:'STOCK_ADJUSTMENT',decision:'REQUIRE_MANAGER_APPROVAL',maxOfflineAgeMinutes:240},
  {actionType:'SUPPLIER_PAYMENT',decision:'DENY',maxOfflineAgeMinutes:0},
  {actionType:'USER_SECURITY',decision:'ALLOW',maxOfflineAgeMinutes:120},
  {actionType:'ROLE_ADMINISTRATION',decision:'ALLOW',maxOfflineAgeMinutes:120},
];

function offlinePolicyRules(policy:OfflinePolicyDetail|null):ConfigureOfflinePolicy['rules']{
  const configured=new Map(policy?.rules.map(rule=>[rule.action_type,rule]));
  return OFFLINE_ACTIONS.map(fallback=>{const rule=configured.get(fallback.actionType);return {actionType:fallback.actionType,decision:rule?.decision??fallback.decision,maxOfflineAgeMinutes:rule?.max_offline_age_minutes??fallback.maxOfflineAgeMinutes,constraintsJson:rule?.constraints_json??'{}'};});
}

function offlinePolicyEvidenceState(policy:OfflinePolicyDetail|null):'MISSING'|'ACTIVE'|'STALE'|'EXPIRED'|'REVIEW'{
  if(!policy)return 'MISSING';
  if(policy.state!=='ACTIVE'||policy.device_state!=='ACTIVE'||!policy.synchronized_at||!policy.device_valid_until)return 'REVIEW';
  const now=Date.now();
  if(new Date(policy.valid_until).getTime()<now||new Date(policy.device_valid_until).getTime()<now)return 'EXPIRED';
  if(new Date(policy.synchronized_at).getTime()+policy.max_policy_staleness_minutes*60_000<now)return 'STALE';
  return 'ACTIVE';
}

function OfflinePolicyEditor({policy,onConfigure}:{policy:OfflinePolicyDetail|null;onConfigure:(request:Omit<ConfigureOfflinePolicy,'operationId'>)=>Promise<void>}){
  const [loginWindow,setLoginWindow]=useState(policy?.offline_login_window_minutes??1440);
  const [maxStaleness,setMaxStaleness]=useState(policy?.max_policy_staleness_minutes??1440);
  const [validDays,setValidDays]=useState(30);
  const [rules,setRules]=useState(()=>offlinePolicyRules(policy));
  const [busy,setBusy]=useState(false);
  useEffect(()=>{setLoginWindow(policy?.offline_login_window_minutes??1440);setMaxStaleness(policy?.max_policy_staleness_minutes??1440);setRules(offlinePolicyRules(policy));},[policy]);
  function updateRule(index:number,change:Partial<ConfigureOfflinePolicy['rules'][number]>) {setRules(current=>current.map((rule,position)=>position===index?{...rule,...change}:rule));}
  async function submit(event:FormEvent){event.preventDefault();for(const rule of rules){try{const parsed=JSON.parse(rule.constraintsJson);if(!parsed||Array.isArray(parsed)||typeof parsed!=='object')throw new Error();}catch{window.alert(`${rule.actionType} constraints must be a JSON object`);return;}}setBusy(true);try{await onConfigure({offlineLoginWindowMinutes:loginWindow,maxPolicyStalenessMinutes:maxStaleness,authorizationValidDays:validDays,rules});}finally{setBusy(false);}}
  return <article><span className="eyebrow">Owner or administrator</span><h2>Activate a new immutable version</h2><p>Changing this form retires the prior version; it does not edit historical authority evidence.</p><form className="policy-editor" onSubmit={submit}><div className="policy-window-grid"><label>Offline login window (minutes)<input type="number" min="15" max="10080" step="1" value={loginWindow} onChange={event=>setLoginWindow(event.target.valueAsNumber)} required/></label><label>Maximum policy staleness (minutes)<input type="number" min="15" max="43200" step="1" value={maxStaleness} onChange={event=>setMaxStaleness(event.target.valueAsNumber)} required/></label><label>Authorization validity (days)<input type="number" min="1" max="365" step="1" value={validDays} onChange={event=>setValidDays(event.target.valueAsNumber)} required/></label></div><div className="policy-rule-editor">{rules.map((rule,index)=><div key={rule.actionType}><b>{rule.actionType.replaceAll('_',' ')}</b><select aria-label={`${rule.actionType} decision`} value={rule.decision} onChange={event=>updateRule(index,{decision:event.target.value as OfflinePolicyDecision})}><option value="ALLOW">Allow</option><option value="REQUIRE_MANAGER_APPROVAL">Manager approval</option><option value="DENY">Deny</option></select><input aria-label={`${rule.actionType} maximum offline age`} type="number" min="0" max="43200" step="1" value={rule.maxOfflineAgeMinutes} onChange={event=>updateRule(index,{maxOfflineAgeMinutes:event.target.valueAsNumber})} required/><input aria-label={`${rule.actionType} constraints JSON`} value={rule.constraintsJson} onChange={event=>updateRule(index,{constraintsJson:event.target.value})} required/></div>)}</div><button className="primary" disabled={busy}>{busy?'Activating…':'Activate policy version'}</button></form></article>;
}

function SetupScreen({onReady,setNotice}:{onReady:()=>void;setNotice:(value:string)=>void}){
  const [values,setValues]=useState({businessName:'',branchCode:'MAIN',branchName:'Main Branch',registerCode:'R1',registerName:'Register 1',deviceLabel:'POS-01',ownerEmployeeNo:'OWNER-1',ownerName:'Owner',ownerPin:''});
  async function submit(event:FormEvent){event.preventDefault();try{await posApi.bootstrap(values);onReady();}catch(error){setNotice(message(error));}}
  const field=(key:keyof typeof values,label:string,type='text')=><label>{label}<input type={type} value={values[key]} onChange={event=>setValues(current=>({...current,[key]:event.target.value}))} required/></label>;
  return <main className="auth-screen"><form className="setup-form" onSubmit={submit}><h1>Initialize BHAIPOS</h1><p>Creates the first local business, branch, register, terminal and owner atomically.</p>{field('businessName','Business name')}{field('branchCode','Branch code')}{field('branchName','Branch name')}{field('registerCode','Register code')}{field('registerName','Register name')}{field('deviceLabel','Terminal name')}{field('ownerEmployeeNo','Owner employee number')}{field('ownerName','Owner name')}{field('ownerPin','Owner PIN','password')}<button className="primary">Initialize securely</button></form></main>;
}
