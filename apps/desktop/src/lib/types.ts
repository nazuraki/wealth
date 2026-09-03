export interface Account {
  id: number;
  institution: string;
  account_number_last4: string;
  account_type: string | null;
  display_name: string | null;
  color: string | null;
  closing_balance: number | null;
  statement_period: string | null;
  simplefin_id: string | null;
}

export interface CategoryGroup {
  id: number;
  name: string;
  color: string | null;
  sort_order: number;
  categories: string[];
}
