import { FormEvent, useCallback, useEffect, useRef, useState } from 'react';
import { posApi, type CartSnapshot, type CheckoutResult, type FailedPrintJob, type HeldCart, type OperationalAlert, type Payment, type RefundableSale } from './api/pos';
import { fils, formatBhd, parseBhd, parseQuantityMilli } from './domain/money';

type Phase = 'loading'|'setup'|'login'|'cashier'|'admin';
type Tender = Payment['kind'];

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
  const [lastSale,setLastSale]=useState<CheckoutResult|null>(null);
  const {inputRef,restoreFocus}=useBarcodeFocus(phase==='cashier'&&!busy&&held===null&&refundSale===null&&failedPrints===null);

  useEffect(() => {
    posApi.health().then(health => {setPhase(health.initialized?'login':'setup');setNotice(health.ready?'Offline database ready':'Database integrity check failed');}).catch(error => {setNotice(message(error));setPhase('login');});
  },[]);

  const newCart=useCallback(async () => {
    const created=await posApi.createCart();
    const snapshot=await posApi.cartSnapshot(created.id);
    setCart(snapshot); setSelected(null);
  },[]);

  async function loggedIn(displayName:string,recoveredSession:string|null,viewAlerts:boolean,manageAlerts:boolean){
    setCashierName(displayName); setCashSessionId(recoveredSession); setCanViewAlerts(viewAlerts); setCanManageAlerts(manageAlerts); setPhase('cashier'); setNotice(recoveredSession?'Open shift recovered':'Signed in — open a shift for cash tenders');
    await newCart();
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
      const committed=await posApi.refund({operationId:crypto.randomUUID(),saleId:sale.sale_id,reason:reason.trim(),lines,payments});
      setRefundSale(null); setNotice(`Refund committed · ${formatBhd(fils(committed.total))} BHD`);
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

  async function lock(){await posApi.logout();setCart(null);setCashSessionId(null);setCanViewAlerts(false);setCanManageAlerts(false);setAlerts([]);setPhase('login');setNotice('Terminal locked');}

  if (phase==='loading') return <StatusScreen title="BHAIPOS" detail={notice}/>;
  if (phase==='setup') return <SetupScreen onReady={() => {setPhase('login');setNotice('Business initialized — sign in offline');}} setNotice={setNotice}/>;
  if (phase==='login') return <LoginScreen notice={notice} onLogin={loggedIn} setNotice={setNotice}/>;
  if (phase==='admin') return <AlertCentre alerts={alerts} canManage={canManageAlerts} notice={notice} onTransition={transitionAlert} onRefresh={showAlertCentre} onCashier={()=>{setPhase('cashier');restoreFocus();}} onLock={lock}/>;
  if (!cart) return <StatusScreen title="Preparing cashier" detail={notice}/>;

  return <div className="app-shell">
    <header className="topbar">
      <div><strong>BHAIPOS</strong><span className="branch">Local terminal · schema 0019</span></div>
      <div className="status"><span className="dot"/>{notice}</div>
      <div>{canViewAlerts&&<button onClick={showAlertCentre}>Alert Centre</button>}<button onClick={cashSessionId?undefined:openShift}>{cashSessionId?'Shift Open':'Open Shift'}</button><button onClick={lock}>Lock</button></div>
    </header>
    <main className="cashier-grid">
      <section className="selling-panel">
        <form className="scan-row" onSubmit={scanProduct}><input ref={inputRef} value={scan} onChange={event=>setScan(event.target.value)} aria-label="Scan barcode" placeholder="Scan barcode, SKU or PLU…" autoComplete="off"/><button className="primary" disabled={busy}>Add</button></form>
        <div className="cart-meta"><span>Cart {cart.cart_id.slice(0,8)}</span><span>Walk-in customer</span><span>{cart.lines.length} lines</span></div>
        <div className="line-header"><span>Item</span><span>Qty</span><span>Price</span><span>Total</span></div>
        <div className="lines">{cart.lines.length===0?<div className="empty-cart">Scan an item to begin</div>:cart.lines.map(line=><button key={line.id} className={`line ${selected===line.id?'selected':''}`} onClick={()=>{setSelected(line.id);restoreFocus();}}>
          <span className="product"><b>{line.name}</b><small>{line.barcode??line.sku}</small></span><span>{(line.quantity/1000).toFixed(3)}</span><span>{formatBhd(fils(line.unit_price))}</span><strong>{formatBhd(fils(line.gross))}</strong>
        </button>)}</div>
        <div className="action-grid"><button disabled>Qty</button><button disabled>Discount</button><button disabled>Price Override</button><button disabled>Remove</button><button disabled>Customer</button><button disabled>Note</button><button onClick={holdCurrent}>Hold Cart</button><button onClick={showHeld}>Held Carts</button><button onClick={showRefund}>Refund</button><button disabled>Delivery</button><button onClick={showPrintRecovery}>Reprint</button><button disabled>Drawer</button></div>
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
    {refundSale&&<div className="modal-backdrop"><section className="shift-modal" role="dialog" aria-modal="true" aria-label="Refund sale"><header><div><span className="eyebrow">Original receipt</span><h2>{refundSale.receipt_number}</h2></div><button onClick={()=>{setRefundSale(null);restoreFocus();}}>Close</button></header><p className="recalc-note">Values come from the immutable sale snapshot. Select one line for this refund.</p><div className="recovery-list">{refundSale.lines.filter(line=>line.remaining_quantity>0).map(line=><button key={line.sale_line_id} onClick={()=>refundLine(refundSale,line)}><span><b>{line.product_name}</b><small>Remaining {(line.remaining_quantity/1000).toFixed(3)}</small></span><strong>{formatBhd(fils(line.refundable_total))} BHD</strong></button>)}</div></section></div>}
    {failedPrints!==null&&<div className="modal-backdrop"><section className="shift-modal" role="dialog" aria-modal="true" aria-label="Print recovery"><header><div><span className="eyebrow">Asynchronous receipt printing</span><h2>Failed Prints</h2></div><button onClick={()=>{setFailedPrints(null);restoreFocus();}}>Close</button></header><div className="recovery-list">{failedPrints.length===0?<p>No failed print jobs</p>:failedPrints.map(job=><button key={job.print_job_id} onClick={()=>retryPrint(job.print_job_id)}><span><b>{job.receipt_number??job.document_type}</b><small>{job.last_error} · attempt {job.attempts}</small></span><strong>Retry</strong></button>)}</div></section></div>}
  </div>;
}

function StatusScreen({title,detail}:{title:string;detail:string}){return <main className="auth-screen"><section><h1>{title}</h1><p>{detail}</p></section></main>;}

function LoginScreen({notice,onLogin,setNotice}:{notice:string;onLogin:(name:string,session:string|null,viewAlerts:boolean,manageAlerts:boolean)=>Promise<void>;setNotice:(value:string)=>void}){
  const [employeeNo,setEmployeeNo]=useState('');const [pin,setPin]=useState('');const [busy,setBusy]=useState(false);
  async function submit(event:FormEvent){event.preventDefault();setBusy(true);try{const result=await posApi.login({employeeNo,pin});await onLogin(result.displayName,result.cashSessionId,result.canViewAlerts,result.canManageAlerts);}catch(error){setNotice(message(error));}finally{setBusy(false);}}
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

function SetupScreen({onReady,setNotice}:{onReady:()=>void;setNotice:(value:string)=>void}){
  const [values,setValues]=useState({businessName:'',branchCode:'MAIN',branchName:'Main Branch',registerCode:'R1',registerName:'Register 1',deviceLabel:'POS-01',ownerEmployeeNo:'OWNER-1',ownerName:'Owner',ownerPin:''});
  async function submit(event:FormEvent){event.preventDefault();try{await posApi.bootstrap(values);onReady();}catch(error){setNotice(message(error));}}
  const field=(key:keyof typeof values,label:string,type='text')=><label>{label}<input type={type} value={values[key]} onChange={event=>setValues(current=>({...current,[key]:event.target.value}))} required/></label>;
  return <main className="auth-screen"><form className="setup-form" onSubmit={submit}><h1>Initialize BHAIPOS</h1><p>Creates the first local business, branch, register, terminal and owner atomically.</p>{field('businessName','Business name')}{field('branchCode','Branch code')}{field('branchName','Branch name')}{field('registerCode','Register code')}{field('registerName','Register name')}{field('deviceLabel','Terminal name')}{field('ownerEmployeeNo','Owner employee number')}{field('ownerName','Owner name')}{field('ownerPin','Owner PIN','password')}<button className="primary">Initialize securely</button></form></main>;
}
