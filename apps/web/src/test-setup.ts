/**
 * Test preload (bunfig.toml [test].preload) — registers happy-dom globals and
 * minimal browser-API mocks before every Bun test so component interaction
 * tests (testing-library) can render React.
 *
 * The ResizeObserver + requestAnimationFrame stubs are required by React Flow
 * even for non-canvas interaction tests.
 */
import { GlobalRegistrator } from "@happy-dom/global-registrator";

GlobalRegistrator.register();

class ResizeObserverStub {
    observe() {}
    unobserve() {}
    disconnect() {}
}
globalThis.ResizeObserver ??= ResizeObserverStub as unknown as typeof ResizeObserver;
