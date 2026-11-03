// 頂欄的組織／專案切換器：依組織分組列出我所屬的專案，lobby 代表「整個組織」。
import * as React from 'react';
import { Autocomplete, Box, TextField, Typography } from '@mui/material';
import { styled, lighten } from '@mui/material/styles';
import { useLocation } from 'react-router-dom';
import { useTranslate } from 'react-admin';
import { buildOptions, SelectionOption, selectionStore, useSelectionState } from './store';

const GroupHeader = styled('div')(({ theme }) => ({
  position: 'sticky',
  top: '-8px',
  padding: '4px 10px',
  color: theme.palette.primary.main,
  backgroundColor: lighten(theme.palette.primary.light, 0.85),
  fontWeight: 600,
}));

const GroupItems = styled('ul')({ padding: 0 });

export default function ProjectSelector() {
  const translate = useTranslate();
  const state = useSelectionState();
  const location = useLocation();
  const [input, setInput] = React.useState('');

  React.useEffect(() => {
    selectionStore.load().catch(() => undefined);
  }, []);

  // 網址帶了 ?org=／?project= 就跟著切
  React.useEffect(() => {
    selectionStore.applyUrl({ search: location.search, hash: `#${location.pathname}${location.search}` });
    // 只看 query 變化：切換後換頁不會被舊的 ?org= 拉回去
  }, [location.search]);

  const options = React.useMemo(() => buildOptions(state.orgs), [state.orgs]);
  const sel = state.selection;
  const value = sel ? options.find((o) => o.organizationId === sel.organizationId && o.projectId === sel.projectId) ?? null : null;

  if (!state.loaded) return null;
  if (!options.length) {
    return (
      <Typography variant="body2" sx={{ px: 2, opacity: 0.8 }} data-testid="org-selector-empty">
        {translate('eo.selector.noOrganizations')}
      </Typography>
    );
  }

  const label = (o: SelectionOption) => (o.projectIsDefault || !o.projectId
    ? translate('eo.selector.wholeOrganization')
    : o.projectName || '');

  return (
    <Box sx={{ px: 2, py: '6px', minWidth: 260, maxWidth: 360, flex: '0 1 360px' }}>
      <Autocomplete<SelectionOption, false, true>
        size="small"
        disableClearable
        value={value ?? undefined}
        options={options}
        inputValue={input}
        onInputChange={(_e, v) => setInput(v)}
        onChange={(_e, o) => o && selectionStore.select(o.organizationId, o.projectId)}
        isOptionEqualToValue={(o, v) => o.organizationId === v.organizationId && o.projectId === v.projectId}
        groupBy={(o) => (o.isPersonal ? `${o.organizationName}（${translate('eo.selector.personal')}）` : o.organizationName)}
        getOptionLabel={(o) => `${o.organizationName} / ${label(o)}`}
        renderOption={(props, o) => {
          const { key, ...rest } = props as any;
          return (
            <li key={key} {...rest} data-testid={`org-option-${o.organizationId}-${o.projectId ?? 0}`}>
              {label(o)}
            </li>
          );
        }}
        renderInput={(params) => (
          <TextField
            {...params}
            label={translate('eo.selector.label')}
            sx={{ '& .MuiInputBase-root': { bgcolor: 'background.paper' } }}
            inputProps={{ ...params.inputProps, 'data-testid': 'org-selector' }}
          />
        )}
        renderGroup={(params) => (
          <li key={params.key}>
            <GroupHeader>{params.group}</GroupHeader>
            <GroupItems>{params.children}</GroupItems>
          </li>
        )}
      />
    </Box>
  );
}
