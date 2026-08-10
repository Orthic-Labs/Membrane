import brandJson from '../brand.json' with { type: 'json' };

export const PRODUCT_NAME = brandJson.productName;
export const BUNDLE_IDENTIFIER = brandJson.identifier;
export const DMG_NAME_TEMPLATE = brandJson.dmgNameTemplate;
export default brandJson;
