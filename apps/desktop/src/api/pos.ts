import { invoke } from '@tauri-apps/api/core';

export type Health = { ready: boolean; database: string; schema: string; initialized: boolean; authenticated: boolean };
export type IdResponse = { id: string };
export type LoginResponse = { displayName: string; cashSessionId: string | null; canViewAlerts:boolean; canManageAlerts:boolean; canViewDiagnostics:boolean; canExportDiagnostics:boolean };
export type CartLine = {
  id: string; product_id: string; name: string; sku: string; barcode: string | null;
  quantity: number; unit_price: number; net: number; tax: number; gross: number;
};
export type CartSnapshot = {
  cart_id: string; status: string; note: string | null; lines: CartLine[];
  candidate_subtotal: number; candidate_tax: number; candidate_total: number;
};
export type HeldCart = { cart_id: string; note: string | null; updated_at: string };
export type Payment = { kind: 'Cash'|'Card'|'BenefitPay'|'BankTransfer'|'CustomerCredit'|'Custom'; amountFils: number; tenderedFils?: number; reference?: string };
export type CheckoutResult = { sale_id: string; receipt_number: string; subtotal: number; tax: number; total: number; change: number; receipt_sha256: string };
export type FailedPrintJob = { print_job_id:string; sale_id:string|null; receipt_number:string|null; document_type:string; attempts:number; last_error:string; updated_at:string };
export type RefundLineRequest = { saleLineId:string; quantityMilli:number };
export type RefundableSaleLine = { sale_line_id:string; product_name:string; sold_quantity:number; refunded_quantity:number; remaining_quantity:number; refundable_total:number };
export type RefundableSale = { sale_id:string; receipt_number:string; completed_at:string; lines:RefundableSaleLine[] };
export type RefundQuote = { subtotal:number; tax:number; total:number };
export type RefundResult = { refund_id:string; subtotal:number; tax:number; total:number };
export type OperationalAlert = { alert_id:string; severity:'CRITICAL'|'HIGH'|'MEDIUM'|'LOW'; alert_type:string; status:'NEW'|'ACKNOWLEDGED'|'IN_PROGRESS'; title:string; assigned_user_id:string|null; created_at:string };

export const posApi = {
  health: () => invoke<Health>('health'),
  bootstrap: (request: { businessName:string; branchCode:string; branchName:string; registerCode:string; registerName:string; deviceLabel:string; ownerEmployeeNo:string; ownerName:string; ownerPin:string }) => invoke('bootstrap_local_business',{request}),
  login: (request: { employeeNo:string; pin:string }) => invoke<LoginResponse>('login',{request}),
  logout: () => invoke<void>('logout'),
  rotateLocalDeviceCredential: () => invoke<number>('rotate_local_device_credential'),
  openCashSession: (cashSessionId:string,openingFloatFils:number) => invoke<IdResponse>('open_cash_session',{request:{cashSessionId,openingFloatFils}}),
  createCart: () => invoke<IdResponse>('create_cart'),
  cartSnapshot: (cartId:string) => invoke<CartSnapshot>('cart_snapshot',{request:{cartId}}),
  scanBarcode: (cartId:string,barcode:string,quantityMilli=1000) => invoke<CartSnapshot>('scan_barcode',{request:{cartId,barcode,quantityMilli}}),
  holdCart: (cartId:string,note?:string) => invoke<void>('hold_cart',{request:{cartId,note}}),
  listHeldCarts: () => invoke<HeldCart[]>('list_held_carts'),
  restoreCart: (cartId:string) => invoke<CartSnapshot>('restore_cart',{request:{cartId}}),
  checkout: (cartId:string,operationId:string,payments:Payment[]) => invoke<CheckoutResult>('checkout',{request:{cartId,operationId,payments}}),
  listFailedPrintJobs: () => invoke<FailedPrintJob[]>('list_failed_print_jobs'),
  requeueFailedPrintJob: (printJobId:string) => invoke<void>('requeue_failed_print_job',{request:{printJobId}}),
  findRefundableSale: (receiptNumber:string) => invoke<RefundableSale>('find_refundable_sale',{request:{receiptNumber}}),
  quoteRefund: (saleId:string,lines:RefundLineRequest[]) => invoke<RefundQuote>('quote_refund',{request:{saleId,lines}}),
  refund: (request: { operationId:string; saleId:string; reason:string; lines:RefundLineRequest[]; payments:Payment[] }) => invoke<RefundResult>('refund',{request}),
  recordCashMovement: (request: { operationId:string; kind:'PAID_IN'|'PAID_OUT'|'SAFE_DROP'|'NO_SALE'|'PETTY_CASH'; amountFils:number; reason?:string }) => invoke('record_cash_movement',{request}),
  cashSessionReport: () => invoke('cash_session_report'),
  closeCashSession: (operationId:string,countedCashFils:number) => invoke('close_cash_session',{request:{operationId,countedCashFils}}),
  listOperationalAlerts: () => invoke<OperationalAlert[]>('list_operational_alerts'),
  transitionOperationalAlert: (alertId:string,newStatus:'ACKNOWLEDGED'|'IN_PROGRESS'|'RESOLVED'|'DISMISSED',note?:string) => invoke('transition_operational_alert',{request:{operationId:crypto.randomUUID(),alertId,newStatus,note}}),
};
