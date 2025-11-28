'use strict';

class SMWTransformer {
    _stripSMWProp(prop) {
        return prop.replace(/#1?0#/, '');
    }

    _processDataItem({ type, item }) {
        switch (type) {
            case 1: // Number
                return parseFloat(item);
            case 9: // Property
                return this._stripSMWProp(item);
            default:
                return item;
        }
    }

    _induceItemType([item]) {
        if (!item) return null;
        switch (item.type) {
            case 1: return 'number';
            case 2: return 'string';
            case 9: return 'property';
            default: return null;
        }
    }

    _integrateDataItems(items) {
        const processed = items.map(item => this._processDataItem(item));
        return processed.length === 1 ? processed[0] : processed;
    }

    _parseDataItems(items) {
        return {
            type: this._induceItemType(items),
            prop: this._integrateDataItems(items)
        };
    }

    parseItemList(itemList) {
        const properties = [];
        itemList.forEach(({ property, dataitem }) => {
            if (property.startsWith('_')) return; // Skip internal props
            properties.push([property, this._parseDataItems(dataitem)]);
        });
        return properties;
    }

    parse({ query }) {
        const { subject, data: entities } = query;
        return [this._stripSMWProp(subject), this.parseItemList(entities)];
    }
}

module.exports = SMWTransformer;
