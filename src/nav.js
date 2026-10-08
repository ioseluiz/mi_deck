// @ts-check
/**
 * Estado de navegacion: una pila de superficies mas la pagina activa.
 *
 * Entrar a una carpeta apila, volver desapila. La pagina se reinicia al cambiar
 * de superficie: entrar a una carpeta y aparecer en su pagina 3 desorienta.
 */

export class Nav {
  /** @param {any} deck */
  constructor(deck) {
    this.deck = deck;
    /** @type {string[]} */
    this.stack = [deck.root];
    this.page = 0;
  }

  /** @param {any} deck */
  setDeck(deck) {
    this.deck = deck;
    // Si la superficie actual desaparecio del deck, replegar hasta una valida.
    while (this.stack.length > 1 && !deck.surfaces[this.surfaceId]) {
      this.stack.pop();
    }
    if (!deck.surfaces[this.surfaceId]) {
      this.stack = [deck.root];
    }
    this.page = Math.min(this.page, this.pageCount - 1);
  }

  get surfaceId() {
    return this.stack[this.stack.length - 1];
  }

  get surface() {
    return this.deck.surfaces[this.surfaceId];
  }

  /** 0 en la raiz. Determina si se inyecta la tecla de volver. */
  get depth() {
    return this.stack.length - 1;
  }

  get pageCount() {
    const paginas = this.surface?.pages?.length ?? 0;
    return Math.max(1, paginas);
  }

  /** Botones de la pagina activa. */
  get buttons() {
    return this.surface?.pages?.[this.page]?.buttons ?? [];
  }

  /**
   * Entra a una carpeta.
   * @param {string} surfaceId
   * @returns {boolean} si la navegacion se pudo hacer
   */
  enter(surfaceId) {
    if (!this.deck.surfaces[surfaceId]) return false;
    // Guarda contra ciclos: Rust ya los rechaza al guardar, pero un deck editado
    // a mano puede traerlos y colgar la navegacion.
    if (this.stack.includes(surfaceId)) return false;
    this.stack.push(surfaceId);
    this.page = 0;
    return true;
  }

  /** @returns {boolean} si habia a donde volver */
  back() {
    if (this.stack.length <= 1) return false;
    this.stack.pop();
    this.page = 0;
    return true;
  }

  /**
   * Salta directo a un nivel de las migas, sin desapilar una por una.
   * @param {number} index
   */
  jumpTo(index) {
    if (index < 0 || index >= this.stack.length - 1) return false;
    this.stack.length = index + 1;
    this.page = 0;
    return true;
  }

  /** @param {number} delta */
  movePage(delta) {
    const destino = this.page + delta;
    if (destino < 0 || destino >= this.pageCount) return false;
    this.page = destino;
    return true;
  }

  /** Migas de pan, de la raiz a la superficie actual. */
  trail() {
    return this.stack.map((id, i) => ({
      id,
      index: i,
      name: this.deck.surfaces[id]?.name ?? id,
      actual: i === this.stack.length - 1,
    }));
  }
}
