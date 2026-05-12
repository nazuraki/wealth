export interface Account {
  id: number;
  institution: string;
  account_number_last4: string;
  account_type: string | null;
  display_name: string | null;
  color: string | null;
  closing_balance: number | null;
  statement_period: string | null;
}
