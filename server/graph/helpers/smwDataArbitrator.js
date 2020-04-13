class SMWDataArbitrator {
    constructor() {
        //
    }

    _stripSMWProp(prop) {
        return prop.replace(/#1?0#/, '');
    }

    _processDataItem({type, item}) {
        switch (type) {
            case 1:
                return parseFloat(item);

            case 9:
                return this._stripSMWProp(item);

            default:
                return item;
        }
    }

    _induceItemType([item]) {
        const {type} = item;

        switch (type) {
            case 1:
                return 'number';

            case 2:
                return 'string';

            case 9:
                return 'property';

            /* not implemented */
            default:
                return null;
        }
    }

    _flattenIfNeeded(list) {
        if (list.length === 1) {
            return list[0];
        }

        return list;
    }

    _integrateDataItems(items) {
        return this._flattenIfNeeded(items.map(item => this._processDataItem(item)));
    }

    _parseDataItems(items) {
        return {
            type: this._induceItemType(items),
            prop: this._integrateDataItems(items)
        };
    }

    _skipPropertyIfNeeded(propName) {
        return propName[0] === '_';
    }

    parseItemList(itemList) {
        const properties = [];

        itemList.forEach(({property, dataitem}) => {
            if (this._skipPropertyIfNeeded(property)) {
                return;
            }

            properties.push([property, this._parseDataItems(dataitem)]);
        });

        return properties;
    }

    parse({query}) {
        const {subject, data: entities} = query;

        return [this._stripSMWProp(subject), this.parseItemList(entities)];
    }
}

module.exports = SMWDataArbitrator;
