/**
 * mdbook-excalidraw - Mermaid to Excalidraw converter for mdBook
 *
 * This script renders Mermaid diagrams with zoom controls.
 */

(function() {
    'use strict';

    console.log('[mdbook-excalidraw] Initializing...');

    // Decode HTML entities
    function decodeHTMLEntities(text) {
        const textarea = document.createElement('textarea');
        textarea.innerHTML = text;
        return textarea.value;
    }

    // Initialize excalidraw containers
    function initExcalidraw() {
        const containers = document.querySelectorAll('.excalidraw-container');
        console.log(`[mdbook-excalidraw] Found ${containers.length} diagrams`);

        containers.forEach((container, index) => {
            try {
                const mermaidData = container.getAttribute('data-mermaid');

                if (!mermaidData) {
                    console.warn('[mdbook-excalidraw] No mermaid data found for container:', container);
                    return;
                }

                // Decode HTML entities (including &#10; newlines)
                const decodedMermaid = decodeHTMLEntities(mermaidData).replace(/&#10;/g, '\n');

                // Create zoom wrapper
                const zoomWrapper = document.createElement('div');
                zoomWrapper.className = 'excalidraw-zoom-wrapper';

                // Create mermaid preview
                const preview = document.createElement('div');
                preview.className = 'mermaid-preview';
                preview.textContent = decodedMermaid;

                // Replace loading with preview
                container.innerHTML = '';
                zoomWrapper.appendChild(preview);
                container.appendChild(zoomWrapper);

                // Zoom state
                let zoomLevel = 1.0;
                const zoomStep = 0.2;
                const minZoom = 0.5;
                const maxZoom = 3.0;

                // Zoom controls
                const zoomControls = document.createElement('div');
                zoomControls.className = 'excalidraw-zoom-controls';

                const zoomOutBtn = document.createElement('button');
                zoomOutBtn.className = 'excalidraw-zoom-btn';
                zoomOutBtn.textContent = '−';
                zoomOutBtn.title = 'Zoom Out (Ctrl/Cmd + -)';
                zoomOutBtn.onclick = () => {
                    if (zoomLevel > minZoom) {
                        zoomLevel = Math.max(minZoom, zoomLevel - zoomStep);
                        zoomWrapper.style.transform = `scale(${zoomLevel})`;
                        zoomLevelDisplay.textContent = `${Math.round(zoomLevel * 100)}%`;
                    }
                };

                const zoomLevelDisplay = document.createElement('span');
                zoomLevelDisplay.className = 'excalidraw-zoom-level';
                zoomLevelDisplay.textContent = '100%';

                const zoomInBtn = document.createElement('button');
                zoomInBtn.className = 'excalidraw-zoom-btn';
                zoomInBtn.textContent = '+';
                zoomInBtn.title = 'Zoom In (Ctrl/Cmd + +)';
                zoomInBtn.onclick = () => {
                    if (zoomLevel < maxZoom) {
                        zoomLevel = Math.min(maxZoom, zoomLevel + zoomStep);
                        zoomWrapper.style.transform = `scale(${zoomLevel})`;
                        zoomLevelDisplay.textContent = `${Math.round(zoomLevel * 100)}%`;
                    }
                };

                const zoomResetBtn = document.createElement('button');
                zoomResetBtn.className = 'excalidraw-zoom-btn';
                zoomResetBtn.textContent = '⟲';
                zoomResetBtn.title = 'Reset Zoom (Ctrl/Cmd + 0)';
                zoomResetBtn.onclick = () => {
                    zoomLevel = 1.0;
                    zoomWrapper.style.transform = `scale(${zoomLevel})`;
                    zoomLevelDisplay.textContent = '100%';
                };

                zoomControls.appendChild(zoomOutBtn);
                zoomControls.appendChild(zoomLevelDisplay);
                zoomControls.appendChild(zoomInBtn);
                zoomControls.appendChild(zoomResetBtn);

                // Make the wrapper position relative for absolute positioning of controls
                const wrapper = container.closest('.excalidraw-wrapper');
                if (wrapper) {
                    wrapper.style.position = 'relative';
                }
                container.appendChild(zoomControls);

                // Keyboard shortcuts for zoom
                document.addEventListener('keydown', (e) => {
                    if (!e.ctrlKey && !e.metaKey) return;

                    if (e.key === '+' || e.key === '=') {
                        e.preventDefault();
                        zoomInBtn.click();
                    } else if (e.key === '-' || e.key === '_') {
                        e.preventDefault();
                        zoomOutBtn.click();
                    } else if (e.key === '0') {
                        e.preventDefault();
                        zoomResetBtn.click();
                    }
                });

                // Try to render with mermaid if available
                if (typeof mermaid !== 'undefined') {
                    try {
                        const mermaidDiv = document.createElement('div');
                        mermaidDiv.className = 'mermaid';
                        mermaidDiv.textContent = decodedMermaid;

                        // Clear and rebuild
                        zoomWrapper.innerHTML = '';
                        zoomWrapper.appendChild(mermaidDiv);

                        // Let mermaid render (it's already initialized by mermaid-init.js)
                        // Just trigger a re-render on this specific div
                        if (typeof mermaid.run !== 'undefined') {
                            mermaid.run({ nodes: [mermaidDiv] });
                        } else {
                            // Fallback for older mermaid versions
                            mermaid.init(undefined, mermaidDiv);
                        }
                        console.log(`[mdbook-excalidraw] Rendered diagram ${index} with mermaid.js`);
                    } catch (mermaidError) {
                        console.warn('[mdbook-excalidraw] Mermaid rendering failed:', mermaidError);
                    }
                }

                console.log(`[mdbook-excalidraw] Processed diagram ${index}`);

            } catch (error) {
                console.error('[mdbook-excalidraw] Error processing diagram:', error);
                container.innerHTML = `
                    <div class="excalidraw-error">
                        <strong>Error loading diagram:</strong>
                        <pre>${error.message}</pre>
                        <details>
                            <summary>View Mermaid source</summary>
                            <pre><code>${container.getAttribute('data-mermaid')}</code></pre>
                        </details>
                    </div>
                `;
            }
        });
    }

    // Initialize when page loads
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', initExcalidraw);
    } else {
        initExcalidraw();
    }

    // Re-initialize on theme change (for mdbook)
    const themeButtons = document.querySelectorAll('[id^="mdbook-theme-"]');
    themeButtons.forEach(button => {
        button.addEventListener('click', () => {
            setTimeout(initExcalidraw, 100);
        });
    });

    console.log('[mdbook-excalidraw] Loaded successfully');

})();
