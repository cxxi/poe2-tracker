-- Ventes déclarées par les joueurs : une annonce de leur boutique marquée « vendu » dans l'app
-- (objet, dernier prix demandé, temps en vente). Une par annonce et par installation.
CREATE TABLE sales (
    install_id BIGINT UNSIGNED NOT NULL,
    listing_id CHAR(64) NOT NULL,
    league VARCHAR(64) NOT NULL,
    item_class VARCHAR(64) NOT NULL,
    base VARCHAR(120) NOT NULL,
    amount DOUBLE NOT NULL,
    currency VARCHAR(40) NOT NULL,
    listed_at BIGINT NULL,
    gone_at BIGINT NOT NULL,
    data TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (install_id, listing_id),
    INDEX sales_item (league, item_class, base),
    CONSTRAINT sales_install FOREIGN KEY (install_id) REFERENCES installs (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
