/**
 * @typedef {Object} Account
 * @property {number} id
 * @property {string} institution
 * @property {string} account_number_last4
 */

/**
 * @typedef {Object} Statement
 * @property {number} id
 * @property {number} account_id
 * @property {string} statement_period
 * @property {number|null} opening_balance
 * @property {number|null} closing_balance
 * @property {string} imported_at
 */

/**
 * @typedef {Object} Transaction
 * @property {number} id
 * @property {number} statement_id
 * @property {string} date
 * @property {string} description
 * @property {string} category
 * @property {number} amount
 * @property {'debit'|'credit'} type
 */

export {};
