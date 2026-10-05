/**
 * The brand owl's colour (DEC-452). The logo is the product's own pixel owl, and it wears one of the
 * four feather colours an agent's owl may take, picked at random on every page load. The pick sets
 * `data-owl` on `<html>` from `<head>`, before the body paints, so the server's markup never names a
 * colour and the logo never changes colour after it appears; `globals.css` maps each value to its
 * feathers and beak.
 */
export const BRAND_OWL_COLOURS = 4;

export const BRAND_OWL_SCRIPT = `(function(){try{document.documentElement.dataset.owl=String(1+Math.floor(Math.random()*${BRAND_OWL_COLOURS}))}catch(e){}})();`;
